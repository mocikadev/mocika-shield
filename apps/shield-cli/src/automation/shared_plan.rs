//! 受控共享来源：固定身份与有效参数，不承接交付、取消或 SQL 实现。
use super::{args::Profile, plan, snapshot, CheckError, PlanProtectArgs};
use crate::{
    args::{EnvironmentPolicyArg, ProtectArgs, SignArgs},
    config::CliConfig,
};
use serde::{Deserialize, Serialize};
use shield_services::{
    certificates::{CertificateBinding, CertificateSelection, CertificateStoreState},
    config::ConfigStore,
    paths::ApplicationPaths,
    protect::{resolve_protect_settings, ProtectCertificateSource, ProtectOverrides},
};
use std::path::{Path, PathBuf};

const TOKEN_LIMIT: usize = 4096;

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(super) struct Binding {
    version: u32,
    certificate_binding: CertificateBinding,
    runtime_profile: Profile,
    environment_policy: String,
    signing_versions: [bool; 4],
}

impl Binding {
    #[cfg(feature = "mcp-server")]
    pub fn certificate_id(&self) -> &str {
        self.certificate_binding.certificate_id()
    }
    pub fn decode(value: &str) -> Result<Self, CheckError> {
        if value.is_empty()
            || value.len() > TOKEN_LIMIT
            || !value.len().is_multiple_of(2)
            || !value
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err(invalid());
        }
        let bytes = value
            .as_bytes()
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pair| {
                u8::from_str_radix(std::str::from_utf8(pair).map_err(|_| invalid())?, 16)
                    .map_err(|_| invalid())
            })
            .collect::<Result<Vec<_>, _>>()?;
        let binding: Self = serde_json::from_slice(&bytes).map_err(|_| invalid())?;
        binding.validate()?;
        if binding.encode()? != value {
            return Err(invalid());
        }
        Ok(binding)
    }
    pub fn encode(&self) -> Result<String, CheckError> {
        self.validate()?;
        let bytes = serde_json::to_vec(self).map_err(|_| invalid())?;
        if bytes.len() * 2 > TOKEN_LIMIT {
            return Err(invalid());
        }
        Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
    }
    fn validate(&self) -> Result<(), CheckError> {
        let id = self.certificate_binding.certificate_id();
        if self.version != 1
            || self.certificate_binding.schema_version() != 1
            || id.is_empty()
            || id.len() > 128
            || !id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
            || !matches!(self.environment_policy.as_str(), "compatible" | "strict")
            || !self.signing_versions[..3].iter().any(|value| *value)
            || self.signing_versions[3]
        {
            return Err(invalid());
        }
        Ok(())
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct SharedRequest {
    pub app_home: Option<PathBuf>,
    pub certificate_id: Option<String>,
    pub use_default_certificate: bool,
    pub runtime_profile: Option<Profile>,
    pub environment_policy: Option<String>,
    pub binding: Option<Binding>,
}
impl SharedRequest {
    pub fn paths(&self) -> Result<ApplicationPaths, CheckError> {
        self.app_home
            .as_deref()
            .map_or_else(ApplicationPaths::current_user, ApplicationPaths::isolated)
            .map_err(|_| invalid())
    }
    pub fn validate(&self) -> Result<(), CheckError> {
        self.paths()?;
        if self
            .certificate_id
            .as_ref()
            .is_some_and(|id| id.trim().is_empty() || id.len() > 128)
            || (self.certificate_id.is_some() && self.use_default_certificate)
            || self
                .environment_policy
                .as_deref()
                .is_some_and(|p| !matches!(p, "compatible" | "strict"))
            || (self.binding.is_some()
                && (self.certificate_id.is_some()
                    || self.use_default_certificate
                    || self.runtime_profile.is_some()
                    || self.environment_policy.is_some()))
        {
            return Err(invalid());
        }
        if let Some(binding) = &self.binding {
            binding.validate()?;
        }
        Ok(())
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct SharedIdentity {
    database: PathBuf,
    keystore_dir: PathBuf,
    pub binding: Binding,
}
impl SharedIdentity {
    pub fn recheck(&self) -> Result<(), CheckError> {
        self.binding.validate()?;
        CertificateStoreState::open_read_only(self.database.clone(), self.keystore_dir.clone())
            .map_err(|_| changed())?
            .check_certificate_binding(&self.binding.certificate_binding)
            .map_err(|_| changed())
    }
    pub fn matches(&self, request: &SharedRequest) -> Result<(), CheckError> {
        let paths = request.paths()?;
        if paths.certificate_database() != self.database
            || paths.keystore_dir() != self.keystore_dir
            || request
                .binding
                .as_ref()
                .is_some_and(|expected| expected != &self.binding)
        {
            return Err(changed());
        }
        self.recheck()
    }
}

pub(super) fn prepare(
    mut args: PlanProtectArgs,
    config_path: Option<&Path>,
    request: &SharedRequest,
    workspace: &Path,
) -> Result<plan::Assessment, CheckError> {
    request.validate()?;
    if ["MOCIKA_SHIELD_KS_PASS", "MOCIKA_SHIELD_KEY_PASS"]
        .iter()
        .any(|key| std::env::var_os(key).is_some())
    {
        return Err(CheckError::new(
            "SHARED_CREDENTIAL_CONFLICT",
            "共享证书不接受外部密码环境。",
        ));
    }
    let config_snapshot = config_path
        .map(|path| snapshot::file("config", path))
        .transpose()?;
    let config = CliConfig::load(config_snapshot.as_ref().map(|s| s.lookup_path.as_path()))
        .map_err(|_| invalid())?;
    let project = config.shared_agent_overrides().map_err(|_| invalid())?;
    config
        .require_shared_sign_source(&SignArgs::default())
        .map_err(|_| invalid())?;
    let paths = request.paths()?;
    let (profile, policy, selection) = if let Some(binding) = &request.binding {
        (
            binding.runtime_profile,
            binding.environment_policy.clone(),
            ProtectCertificateSource::Saved(
                binding.certificate_binding.certificate_id().to_owned(),
            ),
        )
    } else {
        let defaults = ConfigStore::new(paths.config_file())
            .read_protect_defaults()
            .map_err(|_| invalid())?;
        let settings = resolve_protect_settings(
            &defaults,
            project,
            ProtectOverrides {
                runtime_mode: request.runtime_profile.map(|p| {
                    match p {
                        Profile::Standard => "standard",
                        Profile::AndroidApi19 => "android_api19",
                    }
                    .into()
                }),
                environment_policy: request.environment_policy.clone(),
                sign_after_protect: Some(true),
                certificate: request
                    .certificate_id
                    .clone()
                    .map(ProtectCertificateSource::Saved)
                    .or_else(|| {
                        request
                            .use_default_certificate
                            .then_some(ProtectCertificateSource::Default)
                    }),
            },
        )
        .map_err(|_| invalid())?;
        (
            match settings.runtime_profile {
                shield_core::RuntimeProfile::Standard => Profile::Standard,
                shield_core::RuntimeProfile::AndroidApi19 => Profile::AndroidApi19,
            },
            match settings.environment_policy {
                shield_core::EnvironmentPolicy::Compatible => "compatible",
                shield_core::EnvironmentPolicy::Strict => "strict",
            }
            .into(),
            settings.certificate,
        )
    };
    let store =
        CertificateStoreState::open_read_only(paths.certificate_database(), paths.keystore_dir())
            .map_err(|_| changed())?;
    if let Some(binding) = &request.binding {
        store
            .check_certificate_binding(&binding.certificate_binding)
            .map_err(|_| changed())?;
    }
    let selection = match &selection {
        ProtectCertificateSource::Saved(id) => CertificateSelection::Id(id),
        ProtectCertificateSource::Default => CertificateSelection::Default,
        ProtectCertificateSource::External => return Err(invalid()),
    };
    let (certificate, record_binding) = store
        .capture_signing_identity(selection)
        .map_err(|_| changed())?
        .into_parts();
    if request
        .binding
        .as_ref()
        .is_some_and(|b| b.certificate_binding != record_binding)
    {
        return Err(changed());
    }
    let mut signing = SignArgs {
        input: Some(args.input.clone()),
        output: Some(args.output.clone()),
        ..Default::default()
    };
    if let Some(binding) = &request.binding {
        let [v1, v2, v3, v4] = binding.signing_versions;
        signing.v1 = Some(v1);
        signing.v2 = Some(v2);
        signing.v3 = Some(v3);
        signing.v4 = Some(v4);
    }
    let sign = config
        .merge_shared_sign(signing, certificate)
        .map_err(|_| invalid())?;
    if sign.v4 {
        return Err(CheckError::new(
            "SIGNING_V4_NOT_SUPPORTED",
            "受控执行不支持 V4，不会静默关闭。",
        ));
    }
    let binding = Binding {
        version: 1,
        certificate_binding: record_binding,
        runtime_profile: profile,
        environment_policy: policy.clone(),
        signing_versions: [sign.v1, sign.v2, sign.v3, sign.v4],
    };
    binding.validate()?;
    args.runtime_profile = profile;
    let protect = config
        .merge_shared_agent_protect(ProtectArgs {
            input: Some(args.input.clone()),
            output: Some(args.output.clone()),
            exclude_abis: args.exclude_abis.clone(),
            environment_policy: Some(if policy == "strict" {
                EnvironmentPolicyArg::Strict
            } else {
                EnvironmentPolicyArg::Compatible
            }),
            ..Default::default()
        })
        .map_err(|_| invalid())?;
    let mut sources = plan::sources(
        config_snapshot,
        &protect,
        &sign,
        profile.resolve().0,
        Some(workspace),
    )?;
    sources.shared = Some(SharedIdentity {
        database: paths.certificate_database(),
        keystore_dir: paths.keystore_dir(),
        binding,
    });
    plan::prepare_resolved(
        args,
        protect,
        sign,
        sources,
        true,
        profile.resolve().1,
        Some(workspace),
    )
}

fn invalid() -> CheckError {
    CheckError::new(
        "SHARED_BINDING_INVALID",
        "共享绑定或配置无效，请重新规划；不输出原始参数。",
    )
}
fn changed() -> CheckError {
    CheckError::new(
        "PLAN_SOURCE_CHANGED",
        "所选证书材料不可用或已变化，请重新规划。",
    )
}

#[cfg(test)]
#[path = "shared_plan_tests.rs"]
mod tests;

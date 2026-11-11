//! 独立签名的私有计划：只解析共享身份和签名材料，不读取加固偏好或壳资源。
use super::{snapshot, CheckError};
use crate::{
    args::SignArgs,
    config::{CliConfig, ResolvedSignArgs},
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use shield_core::{inspect_signing_input, normalize_fingerprint};
use shield_services::{
    certificates::{CertificateBinding, CertificateSelection, CertificateStoreState},
    paths::ApplicationPaths,
};
use std::path::{Path, PathBuf};

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(super) struct Binding {
    certificate: CertificateBinding,
    versions: [bool; 4],
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Request {
    pub app_home: Option<PathBuf>,
    pub certificate_id: Option<String>,
    pub binding: Option<Binding>,
}
impl Request {
    fn paths(&self) -> Result<ApplicationPaths, CheckError> {
        self.app_home
            .as_deref()
            .map_or_else(ApplicationPaths::current_user, ApplicationPaths::isolated)
            .map_err(|_| invalid())
    }
    pub fn validate(&self) -> Result<(), CheckError> {
        self.paths()?;
        if self.certificate_id.as_ref().is_some_and(|id| !valid_id(id))
            || (self.certificate_id.is_some() && self.binding.is_some())
        {
            return Err(invalid());
        }
        if let Some(binding) = &self.binding {
            if binding.certificate.schema_version() != 1
                || !valid_id(binding.certificate.certificate_id())
                || binding.versions[3]
                || !binding.versions[..3].iter().any(|v| *v)
            {
                return Err(invalid());
            }
        }
        Ok(())
    }
}
fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Sources {
    // 独立种类字段使无标签兼容解码不能将签名来源误认作旧加固来源。
    operation: Operation,
    pub config: Option<snapshot::FileSnapshot>,
    database: PathBuf,
    keystore_dir: PathBuf,
    pub binding: Binding,
    pub input: snapshot::FileSnapshot,
    pub keystore: snapshot::FileSnapshot,
    pub java: snapshot::FileSnapshot,
    pub keytool: snapshot::FileSnapshot,
    pub apksigner: snapshot::FileSnapshot,
    pub content_sha256: [u8; 32],
    pub certificate_binding: Option<[u8; 32]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub java_adapter: Option<super::adapter::Binding>,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Operation {
    Sign,
}

impl Sources {
    pub fn files(&self) -> Vec<&snapshot::FileSnapshot> {
        self.config
            .iter()
            .chain([
                &self.input,
                &self.keystore,
                &self.java,
                &self.keytool,
                &self.apksigner,
            ])
            .chain(self.java_adapter.iter().map(|b| b.snapshot()))
            .collect()
    }
    pub fn recheck_record(&self) -> Result<(), CheckError> {
        CertificateStoreState::open_read_only(self.database.clone(), self.keystore_dir.clone())
            .map_err(|_| changed())?
            .check_certificate_binding(&self.binding.certificate)
            .map_err(|_| changed())
    }
    pub fn matches(&self, request: &Request, has_config: bool) -> Result<(), CheckError> {
        request.validate()?;
        super::adapter::require(self.java_adapter.as_ref())?;
        let paths = request.paths()?;
        if paths.certificate_database() != self.database
            || paths.keystore_dir() != self.keystore_dir
            || has_config != self.config.is_some()
            || request.binding.as_ref().is_some_and(|b| *b != self.binding)
            || request
                .certificate_id
                .as_ref()
                .is_some_and(|id| id != self.binding.certificate.certificate_id())
        {
            return Err(changed());
        }
        for (source, role) in self
            .files()
            .iter()
            .zip(self.config.iter().map(|_| "config").chain([
                "input",
                "keystore",
                "java",
                "keytool",
                "apksigner",
            ]))
        {
            if source.role != role {
                return Err(invalid());
            }
        }
        self.recheck_record()
    }
    pub fn materials(&self) -> Vec<(super::task_materials::Role, &snapshot::FileSnapshot)> {
        use super::task_materials::Role;
        let mut values = vec![(Role::Input, &self.input)];
        values.extend(self.config.iter().map(|c| (Role::Config, c)));
        values.extend([
            (Role::Keystore, &self.keystore),
            (Role::Apksigner, &self.apksigner),
        ]);
        values.extend(
            self.java_adapter
                .iter()
                .map(|b| (Role::JavaAdapter, b.snapshot())),
        );
        values
    }
    pub fn material_version(&self) -> u32 {
        if self.java_adapter.is_some() {
            5
        } else {
            3
        }
    }
}

pub(super) struct Prepared {
    pub sources: Sources,
    pub sign: ResolvedSignArgs,
    pub digest: [u8; 32],
    pub certificate: [u8; 32],
    pub warning: bool,
}

pub(super) fn prepare(
    input: &Path,
    output: &Path,
    config_path: Option<&Path>,
    request: &Request,
    workspace: &Path,
) -> Result<Prepared, CheckError> {
    request.validate()?;
    if ["MOCIKA_SHIELD_KS_PASS", "MOCIKA_SHIELD_KEY_PASS"]
        .iter()
        .any(|key| std::env::var_os(key).is_some())
    {
        return Err(CheckError::new(
            "SHARED_CREDENTIAL_CONFLICT",
            "共享签名不接受外部密码环境。",
        ));
    }
    let config_snapshot = config_path
        .map(|p| snapshot::file("config", p))
        .transpose()?;
    let config = CliConfig::load(config_snapshot.as_ref().map(|c| c.lookup_path.as_path()))
        .map_err(|_| invalid())?;
    config
        .require_shared_sign_source(&SignArgs::default())
        .map_err(|_| invalid())?;
    let paths = request.paths()?;
    let store =
        CertificateStoreState::open_read_only(paths.certificate_database(), paths.keystore_dir())
            .map_err(|_| unavailable())?;
    if let Some(binding) = &request.binding {
        store
            .check_certificate_binding(&binding.certificate)
            .map_err(|_| changed())?;
    }
    let id = request
        .binding
        .as_ref()
        .map(|b| b.certificate.certificate_id())
        .or(request.certificate_id.as_deref());
    let selection = id.map_or(CertificateSelection::Default, CertificateSelection::Id);
    let (certificate, record) = store
        .capture_signing_identity(selection)
        .map_err(|_| unavailable())?
        .into_parts();
    if request
        .binding
        .as_ref()
        .is_some_and(|b| b.certificate != record)
    {
        return Err(changed());
    }
    let mut args = SignArgs {
        input: Some(input.to_owned()),
        output: Some(output.to_owned()),
        ..Default::default()
    };
    if let Some(binding) = &request.binding {
        let [v1, v2, v3, v4] = binding.versions;
        args.v1 = Some(v1);
        args.v2 = Some(v2);
        args.v3 = Some(v3);
        args.v4 = Some(v4);
    }
    let sign = config
        .merge_shared_sign(args, certificate)
        .map_err(|_| invalid())?;
    if sign.v4 {
        return Err(CheckError::new(
            "SIGNING_V4_NOT_SUPPORTED",
            "受控签名仅支持 V1–V3，不会自动关闭 V4。",
        ));
    }
    if !(sign.v1 || sign.v2 || sign.v3) {
        return Err(CheckError::new(
            "SIGNING_VERSIONS_INVALID",
            "至少需要一种 V1–V3 签名。",
        ));
    }
    if !output
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("apk"))
    {
        return Err(invalid());
    }
    let target = snapshot::output(output)?;
    let java = shield_core::utils::ensure_keytool()
        .map_err(|_| CheckError::new("TOOL_NOT_FOUND", "签名所需 Java 或 keytool 不可用。"))?;
    let signer = sign
        .apksigner
        .clone()
        .map(Ok)
        .unwrap_or_else(shield_core::utils::find_apksigner)
        .map_err(CheckError::from_core)?;
    #[cfg(windows)]
    let java_adapter = Some(super::adapter::select()?);
    #[cfg(not(windows))]
    let java_adapter: Option<super::adapter::Binding> = None;
    let bound = java_adapter
        .as_ref()
        .map(|binding| super::adapter::Bound::new(binding, &binding.snapshot().path, workspace))
        .transpose()?;
    let keytool = snapshot::file("keytool", java.keytool_path.as_deref().ok_or_else(invalid)?)?;
    let java = snapshot::file("java", java.java_path.as_deref().ok_or_else(invalid)?)?;
    let tools = super::java_tools::Tools::new(&java.path, bound.as_ref().map(|b| &b.context));
    let content = inspect_signing_input(input, || Ok(())).map_err(|_| {
        CheckError::new(
            "SIGNING_INPUT_INVALID",
            "无法确认 APK 内容或加固绑定，不会降级处理。",
        )
    })?;
    let fingerprint = tools
        .certificate(
            &sign.keystore,
            &sign.key_alias,
            &sign.keystore_password,
            super::plan::keystore_type(sign.keystore_type),
        )
        .map_err(CheckError::from_core)?;
    let certificate = certificate_digest(&fingerprint)?;
    if content
        .certificate_binding
        .is_some_and(|binding| binding != certificate)
    {
        return Err(CheckError::new(
            "CERTIFICATE_MISMATCH",
            "证书与加固包绑定不符，拒绝产生无法启动的包。",
        ));
    }
    let checked = tools
        .inspect(input, &signer)
        .map_err(CheckError::from_core)?;
    let warning = if checked.is_signed {
        let previous = tools
            .fingerprint(input, &signer)
            .map_err(CheckError::from_core)?;
        normalize_fingerprint(&previous) != normalize_fingerprint(&fingerprint)
    } else {
        false
    };
    let sources = Sources {
        operation: Operation::Sign,
        config: config_snapshot,
        database: paths.certificate_database(),
        keystore_dir: paths.keystore_dir(),
        binding: Binding {
            certificate: record,
            versions: [sign.v1, sign.v2, sign.v3, sign.v4],
        },
        input: snapshot::file("input", input)?,
        keystore: snapshot::file("keystore", &sign.keystore)?,
        java,
        keytool,
        apksigner: snapshot::file("apksigner", &signer)?,
        content_sha256: content.content_sha256,
        certificate_binding: content.certificate_binding,
        java_adapter,
    };
    sources.matches(request, config_path.is_some())?;
    snapshot::recheck(&sources.files())?;
    if let Some(binding) = &sources.java_adapter {
        binding.recheck(&mut || Ok(()))?;
    }
    if target != snapshot::output(output)? {
        return Err(changed());
    }
    let schema = if sources.java_adapter.is_some() { 6 } else { 4 };
    let identity = serde_json::json!({"plan_schema_version":schema,"operation":"sign","tool_version":env!("CARGO_PKG_VERSION"),
        "sources":sources,"output":target,"certificate_sha256":certificate,"alias":sign.key_alias,"keystore_type":super::plan::keystore_type(sign.keystore_type)});
    let digest = Sha256::digest(serde_json::to_vec(&identity).map_err(|_| invalid())?).into();
    Ok(Prepared {
        sources,
        sign,
        digest,
        certificate,
        warning,
    })
}
fn invalid() -> CheckError {
    CheckError::new(
        "SIGNING_CONFIG_INVALID",
        "签名配置或内部绑定无效，不输出资料原文。",
    )
}
fn changed() -> CheckError {
    CheckError::new("PLAN_SOURCE_CHANGED", "所选证书材料已变化或不可用。")
}
fn unavailable() -> CheckError {
    CheckError::new(
        "CREDENTIAL_UNAVAILABLE",
        "未找到可用共享证书，请预先配置默认项或明确编号。",
    )
}
fn certificate_digest(value: &str) -> Result<[u8; 32], CheckError> {
    let value = normalize_fingerprint(value);
    if value.len() != 64 || !value.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(invalid());
    }
    let mut bytes = [0u8; 32];
    for (byte, pair) in bytes.iter_mut().zip(value.as_bytes().as_chunks::<2>().0) {
        *byte = u8::from_str_radix(std::str::from_utf8(pair).map_err(|_| invalid())?, 16)
            .map_err(|_| invalid())?;
    }
    Ok(bytes)
}

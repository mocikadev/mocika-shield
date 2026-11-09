//! 普通加固的项目／命令适配；合并规则由应用库提供，旧受控命令不走此入口。
use super::*;
use shield_services::{
    certificates::{CertificateStoreState, CertificateSummary},
    config::{ConfigStore, ProtectDefaults},
    paths::ApplicationPaths,
    protect::{resolve_protect_settings, ProtectCertificateSource, ProtectOverrides},
};

impl ProtectArgs {
    pub(super) fn has_application_settings(&self) -> bool {
        self.use_shared_settings.is_some()
            || self.runtime_profile.is_some()
            || self.sign_after_protect.is_some()
            || self.certificate_id.is_some()
            || self.use_default_certificate
            || self.v1.is_some()
            || self.v2.is_some()
            || self.v3.is_some()
            || self.v4.is_some()
    }
}
impl ProtectConfig {
    pub(super) fn has_application_settings(&self) -> bool {
        self.use_shared_settings.is_some()
            || self.runtime_mode.is_some()
            || self.sign_after_protect.is_some()
            || self.certificate_id.is_some()
            || self.use_default_certificate.is_some()
    }
}

impl CliConfig {
    pub(crate) fn resolve_protect(
        &self,
        args: ProtectArgs,
        home: Option<&Path>,
    ) -> Result<ResolvedProtectArgs> {
        if !self.protect.has_application_settings()
            && !args.has_application_settings()
            && home.is_none()
        {
            return self.merge_protect(args);
        }
        let use_shared = args
            .use_shared_settings
            .or(self.protect.use_shared_settings)
            .unwrap_or(home.is_some());
        let shared = if use_shared {
            ConfigStore::new(application_paths(home)?.config_file()).read_protect_defaults()?
        } else {
            ProtectDefaults::default()
        };
        let project_source = source(
            self.protect.certificate_id.clone(),
            self.protect.use_default_certificate.unwrap_or(false),
        )?;
        let command_source = source(args.certificate_id.clone(), args.use_default_certificate)?;
        if command_source.is_some() && args.sign_after_protect == Some(false) {
            bail!("明确选择证书不能同时关闭加固后签名");
        }
        let versions_requested =
            args.v1.is_some() || args.v2.is_some() || args.v3.is_some() || args.v4.is_some();
        let settings = resolve_protect_settings(
            &shared,
            ProtectOverrides {
                runtime_mode: self.protect.runtime_mode.clone(),
                environment_policy: self.protect.environment_policy.clone(),
                sign_after_protect: self
                    .protect
                    .sign_after_protect
                    .or(project_source.as_ref().map(|_| true)),
                certificate: project_source.or_else(|| {
                    self.has_file_signing_material()
                        .then_some(ProtectCertificateSource::External)
                }),
            },
            ProtectOverrides {
                runtime_mode: args.runtime_profile.as_deref().map(|value| {
                    match value {
                        "android-api19" => "android_api19",
                        _ => value,
                    }
                    .to_owned()
                }),
                environment_policy: args.environment_policy.map(|value| {
                    match value {
                        EnvironmentPolicyArg::Compatible => "compatible",
                        EnvironmentPolicyArg::Strict => "strict",
                    }
                    .into()
                }),
                sign_after_protect: args
                    .sign_after_protect
                    .or(command_source.as_ref().map(|_| true)),
                certificate: command_source,
            },
        )?;
        let default_snapshot = if settings.sign_after_protect.is_none() && use_shared {
            default_certificate(&application_paths(home)?)?
        } else {
            None
        };
        let sign = settings.sign_after_protect.unwrap_or_else(|| {
            default_snapshot
                .as_ref()
                .is_some_and(|value| value.auto_sign_enabled)
        });
        // 沿用默认项时，自动签名偏好和证书编号来自同一次查询，不在后续重新选择默认项。
        let certificate_source = match (settings.certificate, default_snapshot.as_ref()) {
            (ProtectCertificateSource::Default, Some(value)) => {
                ProtectCertificateSource::Saved(value.id.clone())
            }
            (source, _) => source,
        };
        if !sign && versions_requested {
            bail!("已关闭加固后签名，不能忽略本次签名版本参数");
        }
        let versions = (args.v1, args.v2, args.v3, args.v4);
        let mut resolved = self.merge_protect_base(args)?;
        resolved.runtime_profile = Some(settings.runtime_profile);
        resolved.environment_policy = match settings.environment_policy {
            shield_core::EnvironmentPolicy::Compatible => EnvironmentPolicyArg::Compatible,
            shield_core::EnvironmentPolicy::Strict => EnvironmentPolicyArg::Strict,
        };
        if sign {
            let mut signing = SignArgs {
                input: Some(resolved.input.clone()),
                output: Some(resolved.output.clone()),
                apksigner: resolved.apksigner.clone(),
                v1: versions.0,
                v2: versions.1,
                v3: versions.2,
                v4: versions.3,
                json: resolved.json,
                ..Default::default()
            };
            let snapshot = match certificate_source {
                ProtectCertificateSource::External => {
                    // 外部材料仍只从显式项目文件及本次宿主环境取得，不落共享库。
                    signing.ks_pass = Some(
                        std::env::var("MOCIKA_SHIELD_KS_PASS")
                            .ok()
                            .filter(|value| !value.is_empty())
                            .context("外部签名材料缺少有效的宿主密码环境")?,
                    );
                    signing.key_pass = match std::env::var("MOCIKA_SHIELD_KEY_PASS") {
                        Ok(value) if !value.is_empty() => Some(value),
                        Ok(_) | Err(std::env::VarError::NotPresent) => None,
                        Err(_) => bail!("签名密码环境编码无效，不会输出原值"),
                    };
                    self.merge_sign(signing)?
                }
                selection => {
                    if std::env::var_os("MOCIKA_SHIELD_KS_PASS").is_some()
                        || std::env::var_os("MOCIKA_SHIELD_KEY_PASS").is_some()
                    {
                        bail!("共享证书不能混用外部密码环境，请由宿主明确选择材料来源");
                    }
                    match selection {
                        ProtectCertificateSource::Saved(id) => signing.certificate_id = Some(id),
                        ProtectCertificateSource::Default => signing.use_default_certificate = true,
                        ProtectCertificateSource::External => unreachable!(),
                    }
                    crate::management::signing::resolve(self, signing, home)?
                }
            };
            if snapshot.v4 {
                bail!("普通加固后签名当前仅交付单 APK，暂不支持 V4 的附加文件；不会静默关闭 V4");
            }
            resolved.apksigner = snapshot.apksigner.clone();
            resolved.signing = Some(snapshot);
        }
        Ok(resolved)
    }

    fn has_file_signing_material(&self) -> bool {
        self.sign.keystore.is_some()
            || self.sign.key_alias.is_some()
            || self.sign.keystore_type.is_some()
    }
}

#[cfg(test)]
#[path = "protect_tests.rs"]
mod tests;

fn source(id: Option<String>, default: bool) -> Result<Option<ProtectCertificateSource>> {
    if id.is_some() && default {
        bail!("同一层配置不能同时指定证书编号和默认证书");
    }
    Ok(id
        .map(ProtectCertificateSource::Saved)
        .or(default.then_some(ProtectCertificateSource::Default)))
}
fn application_paths(home: Option<&Path>) -> Result<ApplicationPaths> {
    home.map_or_else(ApplicationPaths::current_user, ApplicationPaths::isolated)
        .map_err(anyhow::Error::msg)
}
fn default_certificate(paths: &ApplicationPaths) -> Result<Option<CertificateSummary>> {
    match std::fs::symlink_metadata(paths.certificate_database()) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => bail!("无法检查共享证书库，不会猜测自动签名偏好"),
        Ok(_) => {}
    }
    let store =
        CertificateStoreState::open_read_only(paths.certificate_database(), paths.keystore_dir())
            .map_err(|_| anyhow::anyhow!("无法打开共享证书库，不会猜测自动签名偏好"))?;
    Ok(store
        .list_certificates()
        .map_err(|_| anyhow::anyhow!("无法读取共享默认项"))?
        .into_iter()
        .find(|item| item.is_default))
}

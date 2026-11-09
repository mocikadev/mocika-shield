//! 加固业务参数的纯合并；不定位文件、不读取密码、不管理入口任务生命周期。
use crate::config::{ConfigError, ProtectDefaults};
use shield_core::{EnvironmentPolicy, RuntimeProfile};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProtectCertificateSource {
    Saved(String),
    Default,
    External,
}

#[derive(Default)]
pub struct ProtectOverrides {
    pub runtime_mode: Option<String>,
    pub environment_policy: Option<String>,
    pub sign_after_protect: Option<bool>,
    pub certificate: Option<ProtectCertificateSource>,
}

pub struct EffectiveProtectSettings {
    pub runtime_profile: RuntimeProfile,
    pub environment_policy: EnvironmentPolicy,
    /// 未指定时，沿用全局默认证书的自动签名偏好；不是任意证书的偏好。
    pub sign_after_protect: Option<bool>,
    pub certificate: ProtectCertificateSource,
}

pub fn resolve_protect_settings(
    shared: &ProtectDefaults,
    project: ProtectOverrides,
    command: ProtectOverrides,
) -> Result<EffectiveProtectSettings, ConfigError> {
    let runtime = command
        .runtime_mode
        .or(project.runtime_mode)
        .unwrap_or_else(|| shared.runtime_mode.clone());
    let policy = command
        .environment_policy
        .or(project.environment_policy)
        .unwrap_or_else(|| shared.environment_policy.clone());
    let certificate = command
        .certificate
        .or(project.certificate)
        .or_else(|| {
            shared
                .certificate_id
                .clone()
                .map(ProtectCertificateSource::Saved)
        })
        .unwrap_or(ProtectCertificateSource::Default);
    if matches!(&certificate, ProtectCertificateSource::Saved(id) if id.trim().is_empty()) {
        return Err(ConfigError::InvalidValue);
    }
    Ok(EffectiveProtectSettings {
        runtime_profile: match runtime.as_str() {
            "standard" => RuntimeProfile::Standard,
            "android_api19" => RuntimeProfile::AndroidApi19,
            _ => return Err(ConfigError::InvalidValue),
        },
        environment_policy: match policy.as_str() {
            "compatible" => EnvironmentPolicy::Compatible,
            "strict" => EnvironmentPolicy::Strict,
            _ => return Err(ConfigError::InvalidValue),
        },
        sign_after_protect: command
            .sign_after_protect
            .or(project.sign_after_protect)
            .or(shared.sign_after_protect),
        certificate,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn 共享项目和本次按字段覆盖且显式关闭不被回填() {
        let shared = ProtectDefaults {
            runtime_mode: "android_api19".into(),
            environment_policy: "strict".into(),
            sign_after_protect: Some(true),
            certificate_id: Some("saved".into()),
            ..Default::default()
        };
        let result = resolve_protect_settings(
            &shared,
            ProtectOverrides {
                runtime_mode: Some("standard".into()),
                certificate: Some(ProtectCertificateSource::Saved("project".into())),
                ..Default::default()
            },
            ProtectOverrides {
                sign_after_protect: Some(false),
                certificate: Some(ProtectCertificateSource::Default),
                ..Default::default()
            },
        )
        .unwrap();
        assert!(matches!(result.runtime_profile, RuntimeProfile::Standard));
        assert!(matches!(
            result.environment_policy,
            EnvironmentPolicy::Strict
        ));
        assert_eq!(result.sign_after_protect, Some(false));
        assert_eq!(result.certificate, ProtectCertificateSource::Default);
        assert_eq!(shared.certificate_id.as_deref(), Some("saved"));
    }
    #[test]
    fn 保留未指定的自动签名意图并拒绝未知模式策略和空编号() {
        let settings = resolve_protect_settings(
            &ProtectDefaults::default(),
            ProtectOverrides::default(),
            ProtectOverrides::default(),
        )
        .unwrap();
        assert_eq!(settings.sign_after_protect, None);
        for patch in [
            ProtectOverrides {
                runtime_mode: Some("future".into()),
                ..Default::default()
            },
            ProtectOverrides {
                environment_policy: Some("future".into()),
                ..Default::default()
            },
            ProtectOverrides {
                certificate: Some(ProtectCertificateSource::Saved(String::new())),
                ..Default::default()
            },
        ] {
            assert!(resolve_protect_settings(
                &ProtectDefaults::default(),
                patch,
                ProtectOverrides::default()
            )
            .is_err());
        }
    }
}

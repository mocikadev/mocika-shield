use anyhow::{bail, Context, Result};
use serde::Deserialize;
use std::path::{Path, PathBuf};

use crate::args::{EnvironmentPolicyArg, KeystoreTypeArg, ProtectArgs, SignArgs};

mod protect;
#[cfg(test)]
#[path = "config/shared_sign_tests.rs"]
mod shared_sign_tests;

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CliConfig {
    #[serde(default)]
    schema_version: u32,
    #[serde(default)]
    protect: ProtectConfig,
    #[serde(default)]
    sign: SignConfig,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProtectConfig {
    use_shared_settings: Option<bool>,
    runtime_mode: Option<String>,
    sign_after_protect: Option<bool>,
    certificate_id: Option<String>,
    use_default_certificate: Option<bool>,
    input: Option<PathBuf>,
    output: Option<PathBuf>,
    apktool: Option<PathBuf>,
    resources: Option<PathBuf>,
    apksigner: Option<PathBuf>,
    environment_policy: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct SignConfig {
    input: Option<PathBuf>,
    output: Option<PathBuf>,
    keystore: Option<PathBuf>,
    key_alias: Option<String>,
    keystore_type: Option<String>,
    apksigner: Option<PathBuf>,
    v1: Option<bool>,
    v2: Option<bool>,
    v3: Option<bool>,
    v4: Option<bool>,
}

impl CliConfig {
    #[cfg(all(feature = "agent-executor", target_os = "macos"))]
    pub(crate) fn shared_agent_overrides(&self) -> Result<shield_app::protect::ProtectOverrides> {
        use shield_app::protect::{ProtectCertificateSource, ProtectOverrides};
        if self.protect.use_shared_settings == Some(false)
            || self.protect.sign_after_protect == Some(false)
            || (self.protect.certificate_id.is_some()
                && self.protect.use_default_certificate == Some(true))
        {
            bail!("受控共享加固需要明确且一致的共享签名来源");
        }
        Ok(ProtectOverrides {
            runtime_mode: self.protect.runtime_mode.clone(),
            environment_policy: self.protect.environment_policy.clone(),
            sign_after_protect: Some(true),
            certificate: self
                .protect
                .certificate_id
                .clone()
                .map(ProtectCertificateSource::Saved)
                .or_else(|| {
                    (self.protect.use_default_certificate == Some(true))
                        .then_some(ProtectCertificateSource::Default)
                }),
        })
    }

    #[cfg(all(feature = "agent-executor", target_os = "macos"))]
    pub(crate) fn merge_shared_agent_protect(
        &self,
        args: ProtectArgs,
    ) -> Result<ResolvedProtectArgs> {
        self.merge_protect_base(args)
    }

    /// 共享证书作为完整材料来源，项目与本次参数只能覆盖签名版本和工具等非秘密设置。
    pub(crate) fn merge_shared_sign(
        &self,
        args: SignArgs,
        certificate: shield_app::certificates::CertificateRecord,
    ) -> Result<ResolvedSignArgs> {
        self.require_shared_sign_source(&args)?;
        Ok(ResolvedSignArgs {
            input: required_path(args.input.or_else(|| self.sign.input.clone()), "--input")?,
            output: required_path(args.output.or_else(|| self.sign.output.clone()), "--output")?,
            keystore: PathBuf::from(certificate.keystore_path),
            key_alias: certificate.key_alias,
            keystore_password: certificate.keystore_password,
            key_password: certificate.key_password,
            keystore_type: parse_keystore_type(Some(&certificate.ks_type))?
                .context("共享证书类型无效")?,
            apksigner: args.apksigner.or_else(|| self.sign.apksigner.clone()),
            v1: args.v1.or(self.sign.v1).unwrap_or(certificate.sign_v1),
            v2: args.v2.or(self.sign.v2).unwrap_or(certificate.sign_v2),
            v3: args.v3.or(self.sign.v3).unwrap_or(certificate.sign_v3),
            v4: args.v4.or(self.sign.v4).unwrap_or(certificate.sign_v4),
            json: args.json,
        })
    }

    pub(crate) fn require_shared_sign_source(&self, args: &SignArgs) -> Result<()> {
        if args.ks.is_some()
            || args.key_alias.is_some()
            || args.ks_type.is_some()
            || args.ks_pass.is_some()
            || args.key_pass.is_some()
            || self.sign.keystore.is_some()
            || self.sign.key_alias.is_some()
            || self.sign.keystore_type.is_some()
        {
            bail!("共享证书不能与临时 keystore、Alias、类型或密码参数／环境变量混用，请明确选择一种材料来源");
        }
        Ok(())
    }

    pub(crate) fn load(path: Option<&Path>) -> Result<Self> {
        let Some(path) = path else {
            return Ok(Self::default());
        };
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("读取 CLI 配置文件失败: {}", path.display()))?;
        let mut config: Self = toml::from_str(&content)
            .with_context(|| format!("解析 CLI 配置文件失败: {}", path.display()))?;
        if config.schema_version != 1 {
            bail!(
                "不支持的 CLI 配置版本 {}，当前仅支持 1",
                config.schema_version
            );
        }
        let base = path.parent().unwrap_or_else(|| Path::new("."));
        config.resolve_relative_paths(base);
        Ok(config)
    }

    pub(crate) fn merge_protect(&self, args: ProtectArgs) -> Result<ResolvedProtectArgs> {
        // 旧受控规划不具备共享身份绑定，不能因新增项目字段悄悄读取 GUI 数据。
        if self.protect.has_application_settings() || args.has_application_settings() {
            bail!("当前受控规划尚未支持共享加固设置，请使用原配置；不会忽略新字段或读取共享证书库");
        }
        self.merge_protect_base(args)
    }

    fn merge_protect_base(&self, args: ProtectArgs) -> Result<ResolvedProtectArgs> {
        Ok(ResolvedProtectArgs {
            runtime_profile: None,
            signing: None,
            excluded_abis: args.exclude_abis,
            input: required_path(args.input.or_else(|| self.protect.input.clone()), "--input")?,
            output: required_path(
                args.output.or_else(|| self.protect.output.clone()),
                "--output",
            )?,
            apktool: args.apktool.or_else(|| self.protect.apktool.clone()),
            resources: args.resources.or_else(|| self.protect.resources.clone()),
            apksigner: args.apksigner.or_else(|| self.protect.apksigner.clone()),
            environment_policy: args
                .environment_policy
                .or(parse_environment_policy(
                    self.protect.environment_policy.as_deref(),
                )?)
                .unwrap_or_default(),
            json: args.json,
            verbose: args.verbose,
        })
    }

    pub(crate) fn merge_sign(&self, args: SignArgs) -> Result<ResolvedSignArgs> {
        let ks_pass = args
            .ks_pass
            .context("缺少 Keystore 密码，请使用 --ks-pass 或 MOCIKA_SHIELD_KS_PASS")?;
        Ok(ResolvedSignArgs {
            input: required_path(args.input.or_else(|| self.sign.input.clone()), "--input")?,
            output: required_path(args.output.or_else(|| self.sign.output.clone()), "--output")?,
            keystore: required_path(args.ks.or_else(|| self.sign.keystore.clone()), "--ks")?,
            key_alias: args
                .key_alias
                .or_else(|| self.sign.key_alias.clone())
                .context("缺少签名 Alias，请使用 --key-alias 或在配置文件中设置")?,
            key_password: args.key_pass.unwrap_or_else(|| ks_pass.clone()),
            keystore_password: ks_pass,
            keystore_type: args
                .ks_type
                .or(parse_keystore_type(self.sign.keystore_type.as_deref())?)
                .unwrap_or_default(),
            apksigner: args.apksigner.or_else(|| self.sign.apksigner.clone()),
            v1: args.v1.or(self.sign.v1).unwrap_or(true),
            v2: args.v2.or(self.sign.v2).unwrap_or(true),
            v3: args.v3.or(self.sign.v3).unwrap_or(true),
            v4: args.v4.or(self.sign.v4).unwrap_or(false),
            json: args.json,
        })
    }

    fn resolve_relative_paths(&mut self, base: &Path) {
        for path in [
            &mut self.protect.input,
            &mut self.protect.output,
            &mut self.protect.apktool,
            &mut self.protect.resources,
            &mut self.protect.apksigner,
            &mut self.sign.input,
            &mut self.sign.output,
            &mut self.sign.keystore,
            &mut self.sign.apksigner,
        ] {
            if let Some(value) = path.as_mut().filter(|value| value.is_relative()) {
                *value = base.join(&*value);
            }
        }
    }
}

pub(crate) struct ResolvedProtectArgs {
    pub runtime_profile: Option<shield_core::RuntimeProfile>,
    pub signing: Option<ResolvedSignArgs>,
    pub excluded_abis: Vec<String>,
    pub input: PathBuf,
    pub output: PathBuf,
    pub apktool: Option<PathBuf>,
    pub resources: Option<PathBuf>,
    pub apksigner: Option<PathBuf>,
    pub environment_policy: EnvironmentPolicyArg,
    pub json: bool,
    pub verbose: bool,
}

pub(crate) struct ResolvedSignArgs {
    pub input: PathBuf,
    pub output: PathBuf,
    pub keystore: PathBuf,
    pub key_alias: String,
    pub keystore_password: String,
    pub key_password: String,
    pub keystore_type: KeystoreTypeArg,
    pub apksigner: Option<PathBuf>,
    pub v1: bool,
    pub v2: bool,
    pub v3: bool,
    pub v4: bool,
    pub json: bool,
}

fn required_path(value: Option<PathBuf>, flag: &str) -> Result<PathBuf> {
    value.with_context(|| format!("缺少必填参数 {flag}，请通过命令行或配置文件提供"))
}

fn parse_environment_policy(value: Option<&str>) -> Result<Option<EnvironmentPolicyArg>> {
    value
        .map(|value| match value {
            "compatible" => Ok(EnvironmentPolicyArg::Compatible),
            "strict" => Ok(EnvironmentPolicyArg::Strict),
            _ => bail!("environment_policy 仅支持 compatible 或 strict"),
        })
        .transpose()
}

fn parse_keystore_type(value: Option<&str>) -> Result<Option<KeystoreTypeArg>> {
    value
        .map(|value| match value.to_ascii_lowercase().as_str() {
            "jks" => Ok(KeystoreTypeArg::Jks),
            "pkcs12" | "p12" => Ok(KeystoreTypeArg::Pkcs12),
            _ => bail!("keystore_type 仅支持 jks 或 pkcs12"),
        })
        .transpose()
}

#[cfg(test)]
mod tests {
    #[test]
    fn 排除架构参数仅对本次命令有效() {
        use clap::Parser;
        let cli = crate::args::Cli::try_parse_from([
            "shield",
            "protect",
            "-i",
            "input.apk",
            "-o",
            "output.apk",
            "--exclude-abis",
            "mips,mips64,armeabi",
        ])
        .unwrap();
        let crate::args::Commands::Protect(args) = cli.command else {
            panic!("应解析为加固命令")
        };
        assert_eq!(args.exclude_abis, ["mips", "mips64", "armeabi"]);
        assert!(crate::args::ProtectArgs::default().exclude_abis.is_empty());
    }
    use super::*;

    #[test]
    fn 配置相对路径以配置文件目录为基准() {
        let temp = tempfile::tempdir().unwrap();
        let config_path = temp.path().join("shield-cli.toml");
        std::fs::write(
            &config_path,
            r#"
schema_version = 1

[protect]
input = "input.apk"
output = "out/protected.apk"
environment_policy = "strict"
"#,
        )
        .unwrap();

        let config = CliConfig::load(Some(&config_path)).unwrap();
        let resolved = config.merge_protect(ProtectArgs::default()).unwrap();
        assert_eq!(resolved.input, temp.path().join("input.apk"));
        assert_eq!(resolved.output, temp.path().join("out/protected.apk"));
        assert!(matches!(
            resolved.environment_policy,
            EnvironmentPolicyArg::Strict
        ));
    }

    #[test]
    fn 命令行参数覆盖配置文件() {
        let config: CliConfig = toml::from_str(
            r#"
schema_version = 1
[protect]
input = "config.apk"
output = "config-out.apk"
"#,
        )
        .unwrap();
        let args = ProtectArgs {
            input: Some(PathBuf::from("cli.apk")),
            output: Some(PathBuf::from("cli-out.apk")),
            ..ProtectArgs::default()
        };
        let resolved = config.merge_protect(args).unwrap();
        assert_eq!(resolved.input, PathBuf::from("cli.apk"));
        assert_eq!(resolved.output, PathBuf::from("cli-out.apk"));
    }

    #[test]
    fn 签名密码不会从配置文件读取() {
        let error = toml::from_str::<CliConfig>(
            r#"
schema_version = 1
[sign]
input = "input.apk"
output = "signed.apk"
keystore = "release.jks"
key_alias = "release"
keystore_password = "secret"
"#,
        )
        .unwrap_err();
        assert!(error.to_string().contains("unknown field"));
    }

    #[test]
    fn 未知配置版本会被拒绝() {
        let temp = tempfile::tempdir().unwrap();
        let config_path = temp.path().join("shield-cli.toml");
        std::fs::write(&config_path, "schema_version = 2\n").unwrap();
        let error = CliConfig::load(Some(&config_path)).unwrap_err();
        assert!(error.to_string().contains("当前仅支持 1"));
    }
}

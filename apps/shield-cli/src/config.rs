use anyhow::{bail, Context, Result};
use serde::Deserialize;
use std::path::{Path, PathBuf};

use crate::args::{EnvironmentPolicyArg, KeystoreTypeArg, ProtectArgs, SignArgs};

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
        Ok(ResolvedProtectArgs {
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

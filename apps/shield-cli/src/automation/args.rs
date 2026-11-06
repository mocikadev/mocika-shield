use clap::{Args, ValueEnum};
use std::path::PathBuf;

#[derive(Clone, Copy, Default, ValueEnum)]
pub(super) enum Operation {
    #[default]
    Protect,
    Sign,
    Certificate,
}

#[derive(Args)]
pub(crate) struct DoctorArgs {
    /// 纯加固、独立签名或证书操作；自动签名还须满足证书核对依赖。
    #[arg(long, value_enum, default_value = "protect")]
    pub(super) operation: Operation,
    #[arg(long, value_name = "JAR")]
    pub(super) apktool: Option<PathBuf>,
    #[arg(long, value_name = "JAR")]
    pub(super) apksigner: Option<PathBuf>,
    /// 仅检查文件可读，不验证资源协议或切换加固资源。
    #[arg(long, value_name = "ZIP")]
    pub(super) resources: Option<PathBuf>,
    /// 本命令默认即输出版本化 JSON，显式指定时行为相同。
    #[arg(long)]
    pub(super) json: bool,
}

#[derive(Clone, Copy, Default, ValueEnum, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(super) enum Profile {
    #[default]
    Standard,
    AndroidApi19,
}

impl Profile {
    pub(super) fn resolve(self) -> (shield_core::RuntimeProfile, &'static str) {
        match self {
            Self::Standard => (shield_core::RuntimeProfile::Standard, "standard"),
            Self::AndroidApi19 => (shield_core::RuntimeProfile::AndroidApi19, "android-api19"),
        }
    }
}

#[derive(Args)]
pub(crate) struct PreflightArgs {
    #[arg(short, long, value_name = "APK")]
    pub(super) input: PathBuf,
    /// 仅选择预检规则，不切换实际加固资源。
    #[arg(long, value_enum, default_value = "standard")]
    pub(super) runtime_profile: Profile,
    #[arg(long, value_name = "JAR")]
    pub(super) apksigner: Option<PathBuf>,
    /// 计划签名证书的完整 SHA-256，可含冒号或空格。
    #[arg(long, value_parser = fingerprint)]
    pub(super) expected_cert_sha256: Option<String>,
    /// 本命令默认即输出版本化 JSON，显式指定时行为相同。
    #[arg(long)]
    pub(super) json: bool,
}

fn fingerprint(value: &str) -> Result<String, String> {
    let normalized = shield_core::normalize_fingerprint(value);
    if normalized.len() == 64 && normalized.bytes().all(|value| value.is_ascii_hexdigit()) {
        Ok(normalized)
    } else {
        Err("证书指纹必须是完整的 64 位十六进制 SHA-256，可含冒号或空格".to_owned())
    }
}

#[derive(Args)]
pub(crate) struct PlanProtectArgs {
    /// 本次计划的运行系统模式，资源协议和 APK 预检须使用同一模式。
    #[arg(long, value_enum, default_value = "standard")]
    pub(super) runtime_profile: Profile,
    /// 本次原始已签名 APK，覆盖配置中的默认输入。
    #[arg(short, long, value_name = "APK")]
    pub(super) input: PathBuf,
    /// 计划的最终签名 APK，仅检查位置，不生成或覆盖文件。
    #[arg(short, long, value_name = "APK")]
    pub(super) output: PathBuf,
    /// 仅对本次计划有效，不能从预检警告自动推定授权。
    #[arg(long, value_delimiter = ',')]
    pub(super) exclude_abis: Vec<String>,
    /// 本命令默认即输出版本化 JSON，显式指定时行为相同。
    #[arg(long)]
    pub(super) json: bool,
}

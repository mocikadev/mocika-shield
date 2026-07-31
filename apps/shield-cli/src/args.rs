use clap::{Args, Parser, Subcommand, ValueEnum};
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, Default, ValueEnum)]
pub(crate) enum EnvironmentPolicyArg {
    #[default]
    Compatible,
    Strict,
}

#[derive(Clone, Copy, Debug, Default, ValueEnum)]
pub(crate) enum KeystoreTypeArg {
    #[default]
    Jks,
    Pkcs12,
}

#[derive(Parser)]
#[command(
    name = "shield",
    version,
    author,
    about = "Android APK 加固工具",
    arg_required_else_help = true
)]
pub(crate) struct Cli {
    /// CLI 人工配置文件，建议命名为 shield-cli.toml。
    #[arg(long, global = true, value_name = "FILE")]
    pub config: Option<PathBuf>,
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub(crate) enum Commands {
    Protect(ProtectArgs),
    Sign(SignArgs),
    CheckApk {
        path: PathBuf,
    },
    CheckKeystore {
        #[arg(long)]
        ks: PathBuf,
        #[arg(long)]
        alias: String,
        #[arg(long)]
        ks_pass: String,
    },
}

impl Commands {
    pub(crate) fn machine_output(&self) -> bool {
        match self {
            Self::Protect(args) => args.json,
            Self::Sign(args) => args.json,
            Self::CheckApk { .. } | Self::CheckKeystore { .. } => true,
        }
    }
}

#[derive(Args, Default)]
pub(crate) struct ProtectArgs {
    #[arg(short, long, value_name = "APK")]
    pub input: Option<PathBuf>,
    #[arg(short, long, value_name = "APK")]
    pub output: Option<PathBuf>,
    #[arg(long, value_name = "JAR")]
    pub apktool: Option<PathBuf>,
    /// 内部回归测试使用的运行时资源包。
    #[arg(long, value_name = "ZIP", hide = true)]
    pub resources: Option<PathBuf>,
    #[arg(long, value_name = "JAR")]
    pub apksigner: Option<PathBuf>,
    /// 运行时环境策略：兼容模式仅执行反调试，严格模式额外拒绝高置信 Root 环境。
    #[arg(long, value_enum)]
    pub environment_policy: Option<EnvironmentPolicyArg>,
    /// 每行输出一个 JSON 事件，兼容原有 --json-progress 参数。
    #[arg(long, alias = "json-progress")]
    pub json: bool,
    #[arg(short, long)]
    pub verbose: bool,
}

#[derive(Args, Default)]
pub(crate) struct SignArgs {
    #[arg(short, long, value_name = "APK")]
    pub input: Option<PathBuf>,
    #[arg(short, long, value_name = "APK")]
    pub output: Option<PathBuf>,
    #[arg(long, value_name = "KEYSTORE")]
    pub ks: Option<PathBuf>,
    #[arg(long)]
    pub key_alias: Option<String>,
    /// Keystore 密码；自动化环境优先使用 MOCIKA_SHIELD_KS_PASS。
    #[arg(long, env = "MOCIKA_SHIELD_KS_PASS", hide_env_values = true)]
    pub ks_pass: Option<String>,
    /// Key 密码；未提供时沿用 Keystore 密码。
    #[arg(long, env = "MOCIKA_SHIELD_KEY_PASS", hide_env_values = true)]
    pub key_pass: Option<String>,
    #[arg(long, value_enum)]
    pub ks_type: Option<KeystoreTypeArg>,
    #[arg(long, value_name = "JAR")]
    pub apksigner: Option<PathBuf>,
    #[arg(long)]
    pub v1: Option<bool>,
    #[arg(long)]
    pub v2: Option<bool>,
    #[arg(long)]
    pub v3: Option<bool>,
    #[arg(long)]
    pub v4: Option<bool>,
    /// 每行输出一个 JSON 事件。
    #[arg(long)]
    pub json: bool,
}

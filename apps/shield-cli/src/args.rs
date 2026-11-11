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
    /// 资料管理／共享证书签名的隔离根目录，包含 config/ 和 data/；默认共用 GUI 目录。
    #[arg(long, global = true, value_name = "DIRECTORY")]
    pub app_home: Option<PathBuf>,
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub(crate) enum Commands {
    /// 管理与 GUI 共用的本地证书，输出不含密码的版本化 JSON。
    Cert(crate::management::CertificateArgs),
    /// 查询或保存共享业务偏好；不是项目 shield-cli.toml。
    Config(crate::management::ConfigArgs),
    #[cfg(all(
        feature = "mcp-server",
        any(target_os = "macos", target_os = "linux", windows)
    ))]
    /// 本机 MCP 实验入口：默认只读，标准输出由协议独占。
    Mcp(crate::mcp::McpArgs),
    #[cfg(all(
        feature = "agent-executor",
        any(target_os = "macos", target_os = "linux", windows)
    ))]
    /// 受控本地自动化（开发特性）：逐行 JSON，保留旧命令行为。
    Agent(crate::automation::execution::AgentArgs),
    #[cfg(all(
        feature = "agent-executor",
        any(target_os = "macos", target_os = "linux", windows)
    ))]
    #[command(name = "__agent-probe", hide = true)]
    AgentProbe(crate::automation::execution::ProbeArgs),
    #[cfg(all(
        feature = "agent-executor",
        any(target_os = "macos", target_os = "linux", windows)
    ))]
    #[command(name = "__agent-worker", hide = true)]
    AgentWorker(crate::automation::execution::WorkerArgs),
    /// 只读检查指定操作的本机依赖，输出版本化 JSON。
    Doctor(crate::automation::DoctorArgs),
    /// 只读预检 APK，不执行加固、签名或资源切换，输出版本化 JSON。
    Preflight(crate::automation::PreflightArgs),
    /// 只读规划加固后签名：核对本地签名配置、输出位置和文件身份，不执行加固。
    PlanProtect(crate::automation::PlanProtectArgs),
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
            Self::Cert(_) | Self::Config(_) => true,
            #[cfg(all(
                feature = "mcp-server",
                any(target_os = "macos", target_os = "linux", windows)
            ))]
            Self::Mcp(_) => false,
            #[cfg(all(
                feature = "agent-executor",
                any(target_os = "macos", target_os = "linux", windows)
            ))]
            Self::Agent(_) | Self::AgentProbe(_) | Self::AgentWorker(_) => true,
            Self::Doctor(_) | Self::Preflight(_) | Self::PlanProtect(_) => true,
            Self::Protect(args) => args.json,
            Self::Sign(args) => args.json,
            Self::CheckApk { .. } | Self::CheckKeystore { .. } => true,
        }
    }
}

#[derive(Args, Default)]
pub(crate) struct ProtectArgs {
    /// 显式采用与 GUI 共用的加固偏好；可传 false 覆盖项目设置。
    #[arg(long, num_args = 0..=1, default_missing_value = "true")]
    pub use_shared_settings: Option<bool>,
    #[arg(long, value_parser = ["standard", "android-api19"])]
    pub runtime_profile: Option<String>,
    #[arg(long)]
    pub sign_after_protect: Option<bool>,
    /// 明确选择已保存证书，同时启用加固后签名。
    #[arg(long, conflicts_with = "use_default_certificate")]
    pub certificate_id: Option<String>,
    #[arg(long)]
    pub use_default_certificate: bool,
    #[arg(long)]
    pub v1: Option<bool>,
    #[arg(long)]
    pub v2: Option<bool>,
    #[arg(long)]
    pub v3: Option<bool>,
    #[arg(long)]
    pub v4: Option<bool>,
    /// 明确排除不需要且不受支持的 ABI（逗号分隔），仅对本次任务有效。
    #[arg(long, value_delimiter = ',')]
    pub exclude_abis: Vec<String>,
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
    /// 使用与 GUI 共用的已保存证书；不可混用临时 keystore 材料。
    #[arg(long, conflicts_with = "use_default_certificate")]
    pub certificate_id: Option<String>,
    /// 明确使用共享库的全局默认证书；未设置时停止，不猜测其他证书。
    #[arg(long)]
    pub use_default_certificate: bool,
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

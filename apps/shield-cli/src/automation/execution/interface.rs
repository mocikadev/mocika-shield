//! 单任务自动化入口：适配参数与安全结果，不复制执行规则或存储实现。
use super::*;
use clap::Subcommand;
use serde_json::Value;
use std::time::Duration;

mod control;
mod output;
mod result;
use control::{Cause, Controls};
use output::Events;
use result::Terminal;

#[derive(Args)]
pub(crate) struct AgentArgs {
    #[command(subcommand)]
    command: AgentCommand,
}

#[derive(Subcommand)]
enum AgentCommand {
    /// 只读预览共享独立签名，不是执行前置步骤。
    SignPlan(SignCommon),
    /// 使用共享证书，一次完成内部规划、独立签名与无覆盖交付。
    Sign(SignCommon),
    /// 在受控工作进程中规划，不生成最终 APK。
    Plan(Common),
    /// 匹配既有计划后，加固、签名并无覆盖交付。
    Protect {
        #[command(flatten)]
        common: Common,
        /// 受控规划返回的完整 SHA-256；不是授权令牌。
        #[arg(long, value_parser = digest)]
        expect_plan_sha256: [u8; 32],
    },
}

#[derive(Args)]
struct SignCommon {
    #[arg(short, long, value_name = "APK")]
    input: PathBuf,
    #[arg(short, long, value_name = "APK")]
    output: PathBuf,
    /// 省略时使用共享默认证书；不会交互询问或猜选其他记录。
    #[arg(long)]
    certificate_id: Option<String>,
    #[arg(long, default_value_t = 1200, value_parser = clap::value_parser!(u64).range(1..=3600))]
    timeout_seconds: u64,
    #[arg(long)]
    control_stdin: bool,
}
impl From<SignCommon> for Common {
    fn from(args: SignCommon) -> Self {
        Self {
            input: args.input,
            output: args.output,
            runtime_profile: None,
            use_shared_settings: true,
            certificate_id: args.certificate_id,
            use_default_certificate: false,
            environment_policy: None,
            shared_binding: None,
            exclude_abis: vec![],
            timeout_seconds: args.timeout_seconds,
            control_stdin: args.control_stdin,
        }
    }
}

#[derive(Args)]
struct Common {
    #[arg(short, long, value_name = "APK")]
    input: PathBuf,
    #[arg(short, long, value_name = "APK")]
    output: PathBuf,
    #[arg(long, value_enum)]
    runtime_profile: Option<Profile>,
    /// 显式使用同一应用的本地共享偏好和证书。
    #[arg(long)]
    use_shared_settings: bool,
    #[arg(long, conflicts_with = "use_default_certificate")]
    certificate_id: Option<String>,
    #[arg(long)]
    use_default_certificate: bool,
    /// 仅共享规划可覆盖；执行时由绑定参数固定。
    #[arg(long)]
    environment_policy: Option<String>,
    /// 共享规划返回的原样绑定参数，不是授权凭据。
    #[arg(long)]
    shared_binding: Option<String>,
    /// 仅对本次授权有效，不从警告自动推定。
    #[arg(long, value_delimiter = ',')]
    exclude_abis: Vec<String>,
    #[arg(long, default_value_t = 1200, value_parser = clap::value_parser!(u64).range(1..=3600))]
    timeout_seconds: u64,
    /// 显式启用一行取消控制；输入 EOF 也请求取消。默认不读取 stdin。
    #[arg(long)]
    control_stdin: bool,
}

fn digest(value: &str) -> Result<[u8; 32], String> {
    parse_hex(value).map_err(|_| "计划摘要必须是完整的 64 位十六进制 SHA-256".to_owned())
}

pub(crate) fn run_agent(args: AgentArgs, config: Option<&Path>, home: Option<&Path>) -> i32 {
    let (common, mode, operation, signing) = match args.command {
        AgentCommand::SignPlan(common) => (common.into(), RunMode::Plan, "agent_sign_plan", true),
        AgentCommand::Sign(common) => (common.into(), RunMode::Probe, "agent_sign", true),
        AgentCommand::Plan(common) => (common, RunMode::Plan, "agent_plan", false),
        AgentCommand::Protect {
            common,
            expect_plan_sha256,
        } => (
            common,
            RunMode::Protect(expect_plan_sha256),
            "agent_protect",
            false,
        ),
    };
    let started = Instant::now();
    let controls = match Controls::new(common.control_stdin) {
        Ok(controls) => controls,
        Err(()) => return output::initial_failure(operation, "SIGNAL_SETUP_FAILED"),
    };
    let mut random = [0u8; 16];
    if std::fs::File::open("/dev/urandom")
        .and_then(|mut file| file.read_exact(&mut random))
        .is_err()
    {
        return output::initial_failure(operation, "TASK_ID_UNAVAILABLE");
    }
    let Ok(mut events) = Events::new(operation, Some(hex(&random)), controls.clone_signal()) else {
        return 3;
    };
    events.started();
    let shared = common.use_shared_settings
        || home.is_some()
        || common.certificate_id.is_some()
        || common.use_default_certificate
        || common.shared_binding.is_some();
    let mut terminal = if (matches!(mode, RunMode::Plan) && common.shared_binding.is_some())
        || (matches!(mode, RunMode::Protect(_)) && shared && common.shared_binding.is_none())
        || (!shared && common.environment_policy.is_some())
    {
        Terminal::before_start("SHARED_BINDING_INVALID")
    } else {
        let binding = common
            .shared_binding
            .as_deref()
            .map(crate::automation::shared_plan::Binding::decode)
            .transpose();
        match binding {
            Err(err) => Terminal::before_start(err.code),
            Ok(binding) => {
                let mut request = Request {
                    config: config.map(Path::to_owned),
                    sign: signing.then(|| crate::automation::sign_plan::Request {
                        app_home: home.map(Path::to_owned),
                        certificate_id: common.certificate_id.clone(),
                        binding: None,
                    }),
                    shared: (shared && !signing).then_some(
                        crate::automation::shared_plan::SharedRequest {
                            app_home: home.map(Path::to_owned),
                            certificate_id: common.certificate_id,
                            use_default_certificate: common.use_default_certificate,
                            runtime_profile: common.runtime_profile,
                            environment_policy: common.environment_policy,
                            binding,
                        },
                    ),
                    input: common.input,
                    output: common.output,
                    runtime_profile: common.runtime_profile.unwrap_or_default(),
                    exclude_abis: common.exclude_abis,
                    timeout_seconds: common.timeout_seconds,
                };
                match request.validate() {
                    Err(err) => Terminal::before_start(err.code),
                    Ok(()) => Terminal::from_operation(
                        controller::run_controlled(request, controls.cancel(), mode, |notice| {
                            events.progress(notice)
                        }),
                        matches!(mode, RunMode::Plan),
                        signing,
                    ),
                }
            }
        }
    };
    terminal.elapsed_ms = started.elapsed().as_millis();
    terminal.apply_cancel(controls.cause());
    let exit = terminal.exit();
    if events.finish(terminal, Duration::from_secs(2)) {
        exit
    } else {
        3
    }
}

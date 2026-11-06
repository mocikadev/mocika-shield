//! 单任务自动化入口：适配参数与安全结果，不复制执行规则或读取 GUI 数据。
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
struct Common {
    #[arg(short, long, value_name = "APK")]
    input: PathBuf,
    #[arg(short, long, value_name = "APK")]
    output: PathBuf,
    #[arg(long, value_enum, default_value = "standard")]
    runtime_profile: Profile,
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

pub(crate) fn run_agent(args: AgentArgs, config: Option<&Path>) -> i32 {
    let (common, mode, operation) = match args.command {
        AgentCommand::Plan(common) => (common, RunMode::Plan, "agent_plan"),
        AgentCommand::Protect {
            common,
            expect_plan_sha256,
        } => (
            common,
            RunMode::Protect(expect_plan_sha256),
            "agent_protect",
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
    let mut terminal = match config {
        None => Terminal::before_start("SIGNING_CONFIG_REQUIRED"),
        Some(config) => {
            let mut request = Request {
                config: config.to_owned(),
                input: common.input,
                output: common.output,
                runtime_profile: common.runtime_profile,
                exclude_abis: common.exclude_abis,
                timeout_seconds: common.timeout_seconds,
            };
            match request.validate() {
                Err(err) => Terminal::before_start(err.code),
                Ok(()) => Terminal::from_run(
                    controller::run_controlled(request, controls.cancel(), mode, |notice| {
                        events.progress(notice)
                    }),
                    matches!(mode, RunMode::Plan),
                ),
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

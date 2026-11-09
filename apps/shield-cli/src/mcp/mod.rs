//! 本机 MCP 启动与连接生命周期；协议工具、会话和进程监督各自拥有状态。
mod bridge;
mod policy;
mod protocol;
mod runner;
mod server;
mod session;
mod tools;
mod transport;

use clap::{Args, Subcommand};
use policy::Policy;
use rmcp::{RoleServer, ServiceExt};
use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::sync::Semaphore;
use tokio_util::sync::CancellationToken;

#[derive(Args)]
pub(crate) struct McpArgs {
    #[command(subcommand)]
    command: McpCommand,
}
#[derive(Subcommand)]
enum McpCommand {
    /// 通过标准输入输出提供工具；默认只读，执行必须由宿主显式开启。
    Serve {
        #[arg(long)]
        input_root: PathBuf,
        #[arg(long)]
        output_root: PathBuf,
        /// 单次调用总期限，不因进度而延长；停止后的有限收尾另计。
        #[arg(long,default_value_t=120,value_parser=clap::value_parser!(u64).range(1..=3600))]
        timeout_seconds: u64,
        /// 仅由宿主配置；不等于任意任务已获用户授权。
        #[arg(long)]
        allow_execution: bool,
        /// 宿主明确选择共享资料；已有证书签名使用库内密码，创建／导入用宿主预置环境。
        #[arg(long)]
        use_shared_settings: bool,
        /// 可重复指定；授权已有证书编号。留空不授权旧证书，本会话新建成功的编号另行加入。
        #[arg(long, requires = "use_shared_settings")]
        allow_certificate: Vec<String>,
    },
}
pub(crate) fn run(args: McpArgs, config: Option<&Path>, home: Option<&Path>) -> i32 {
    let McpCommand::Serve {
        input_root,
        output_root,
        timeout_seconds,
        allow_execution,
        use_shared_settings,
        allow_certificate,
    } = args.command;
    let policy = std::env::current_exe()
        .map_err(|_| "CLI_IDENTITY_INVALID")
        .and_then(|cli| Policy::new(&input_root, &output_root, config, &cli, timeout_seconds))
        .and_then(|policy| {
            let policy = if use_shared_settings {
                policy.with_shared(home, allow_certificate)?
            } else if home.is_some() {
                return Err("SHARED_SOURCE_NOT_ENABLED");
            } else {
                policy
            };
            if allow_execution && !policy.is_shared() {
                policy.config()?;
            }
            Ok(policy)
        });
    let policy = match policy {
        Ok(policy) => Arc::new(policy),
        Err(code) => {
            eprintln!("MCP 启动被阻断：{code}");
            return 1;
        }
    };
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(_) => {
            eprintln!("MCP 运行环境无法初始化");
            return 1;
        }
    };
    let result = runtime.block_on(serve(policy, allow_execution));
    // macOS 标准流底层可能仍有阻塞线程，不使其阻止已完成业务清场后的退出。
    runtime.shutdown_timeout(Duration::from_secs(1));
    result
}
async fn serve(policy: Arc<Policy>, allow_execution: bool) -> i32 {
    let shutdown = CancellationToken::new();
    let gate = Arc::new(Semaphore::new(1));
    let monitor = tokio::spawn(transport::monitor(shutdown.clone()));
    let server = server::Server {
        policy,
        gate: gate.clone(),
        shutdown: shutdown.clone(),
        session: Arc::new(Mutex::new(session::Session::default())),
        allow_execution,
    };
    let (input, output) = rmcp::transport::stdio();
    let input = transport::DisconnectReader {
        input,
        shutdown: shutdown.clone(),
    };
    let output = transport::DeadlineWriter::new(output, shutdown.clone());
    let transport =
        rmcp::transport::async_rw::AsyncRwTransport::<RoleServer, _, _>::new(input, output)
            .with_max_line_length(64 * 1024);
    let result = tokio::select! {
        _=shutdown.cancelled()=>None,
        result=tokio::time::timeout(Duration::from_secs(20),server.serve_with_ct(transport,shutdown.clone()))=>result.ok().and_then(|value|value.ok()),
    };
    let success = match result {
        Some(service) => service.waiting().await.is_ok(),
        None => false,
    };
    shutdown.cancel();
    // SDK 放弃响应不等于任务已经停止；后台任务持有许可直到状态记录和清场结束。
    let idle = tokio::time::timeout(Duration::from_secs(20), gate.acquire())
        .await
        .is_ok();
    monitor.abort();
    if success && idle {
        0
    } else {
        1
    }
}

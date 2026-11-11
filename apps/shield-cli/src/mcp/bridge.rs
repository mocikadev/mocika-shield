//! 单个 CLI 子进程的有界监督；MCP 不拥有 APK 提交权，也不执行 shell。
use super::policy::{Policy, Result};
use super::protocol::{Progress, Stream};
use crate::automation::native_group::{CommandSpec, ManagedGroup};
use std::ffi::OsString;
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::time::{sleep, Instant};
use tokio_util::sync::CancellationToken;

pub(super) struct Invocation {
    pub arguments: Vec<OsString>,
    pub operation: &'static str,
    pub controlled: bool,
    /// 仅固定的证书创建／导入命令可使用宿主预置凭据，不由工具参数设置。
    pub host_credentials: bool,
}

pub(super) struct Outcome {
    pub exit: i32,
    pub bytes: Vec<u8>,
    pub stop_reason: Option<&'static str>,
}

pub(super) async fn run(
    policy: &Policy,
    call: Invocation,
    deadline: Option<Instant>,
    cancelled: CancellationToken,
    shutdown: CancellationToken,
    mut observe: impl FnMut(Progress) + Send,
) -> Result<Outcome> {
    policy.check()?;
    if cancelled.is_cancelled() || shutdown.is_cancelled() {
        return Err("TASK_CANCELLED");
    }
    if deadline.is_some_and(|deadline| Instant::now() >= deadline) {
        return Err("TASK_TIMEOUT");
    }
    let mut command = CommandSpec::new(policy.cli(), call.arguments, policy.working_dir());
    for name in [
        "JAVA_TOOL_OPTIONS",
        "_JAVA_OPTIONS",
        "JDK_JAVA_OPTIONS",
        "RUST_LOG",
    ] {
        command.remove_env(name);
    }
    if !call.host_credentials && (!call.controlled || policy.is_shared()) {
        command.remove_env("MOCIKA_SHIELD_KS_PASS");
        command.remove_env("MOCIKA_SHIELD_KEY_PASS");
    }
    let mut group = ManagedGroup::spawn_command(command).map_err(|_| "CLI_START_FAILED")?;
    if !call.controlled {
        group.close_input();
    }
    let stdout = group.take_stdout().ok_or("CLI_START_FAILED")?;
    let stdout = super::transport::from_output(stdout);
    let (progress, mut updates) = tokio::sync::mpsc::channel(16);
    let mut reader = tokio::spawn(read_bounded(
        stdout,
        call.controlled.then_some(call.operation),
        progress,
    ));
    let mut bytes = None;
    // 旧单次调用保留原计时边界；只有新签名组合调用传入共同的绝对期限。
    let deadline =
        deadline.unwrap_or_else(|| Instant::now() + Duration::from_secs(policy.timeout_seconds));
    let mut stop: Option<(&'static str, Instant)> = None;
    loop {
        while let Ok(progress) = updates.try_recv() {
            observe(progress);
        }
        if reader.is_finished() && bytes.is_none() {
            match (&mut reader).await {
                Ok(Ok(data)) => bytes = Some(data),
                _ => {
                    stop.get_or_insert(("CLI_OUTPUT_INVALID", Instant::now()));
                    bytes = Some(Vec::new());
                }
            }
        }
        // 不能 try_wait 提前回收组长；PID 复用后再清组可能命中无关任务。
        if group.leader_exited().map_err(|_| "STOP_UNCONFIRMED")? {
            break;
        }
        if stop.is_none() {
            let cause = if cancelled.is_cancelled() || shutdown.is_cancelled() {
                Some("TASK_CANCELLED")
            } else if Instant::now() >= deadline {
                Some("TASK_TIMEOUT")
            } else {
                None
            };
            if let Some(cause) = cause {
                stop = Some((cause, Instant::now()));
            }
        }
        if let Some((_, started)) = stop {
            if call.controlled {
                // 关闭专用控制管道，请求既有父端取消整项任务；不是杀父进程代替清场。
                group.close_input();
                if started.elapsed() > Duration::from_secs(15) {
                    let _ = group.stop(Duration::from_secs(2));
                    reader.abort();
                    return Err("STOP_UNCONFIRMED");
                }
            } else {
                // 普通检查没有工作租约，复用原生整组停止和查询，不把直属进程退出当清场。
                if group.stop(Duration::from_secs(2)).is_err() {
                    reader.abort();
                    return Err("STOP_UNCONFIRMED");
                }
                break;
            }
        }
        sleep(Duration::from_millis(10)).await;
    }
    if group.stop(Duration::from_secs(2)).is_err() {
        reader.abort();
        return Err("STOP_UNCONFIRMED");
    }
    let exit = group.exit_status().ok_or("CLI_EXIT_UNKNOWN")?;
    let bytes = match bytes {
        Some(bytes) => bytes,
        None => match tokio::time::timeout(Duration::from_secs(1), &mut reader).await {
            Ok(Ok(Ok(bytes))) => bytes,
            _ => {
                reader.abort();
                return Err("CLI_OUTPUT_INVALID");
            }
        },
    };
    if let Some((cause, _)) = stop {
        // 受控计划的停止证明必须来自原 CLI 终态，不能因为父进程退出就宣称完整清理。
        if !call.controlled {
            return Err(cause);
        }
    }
    while let Ok(progress) = updates.try_recv() {
        observe(progress);
    }
    Ok(Outcome {
        exit: exit.code().ok_or("CLI_EXIT_UNKNOWN")?,
        bytes,
        stop_reason: stop.map(|(cause, _)| cause),
    })
}

async fn read_bounded(
    mut input: impl AsyncRead + Unpin,
    operation: Option<&'static str>,
    progress: tokio::sync::mpsc::Sender<Progress>,
) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    let mut buffer = [0u8; 4096];
    let mut parsed = 0;
    let mut stream = operation.map(Stream::new);
    loop {
        let count = input
            .read(&mut buffer)
            .await
            .map_err(|_| "CLI_OUTPUT_INVALID")?;
        if count == 0 {
            return Ok(bytes);
        }
        if bytes.len() + count > 128 * 1024 {
            return Err("CLI_OUTPUT_INVALID");
        }
        bytes.extend_from_slice(&buffer[..count]);
        if let Some(stream) = &mut stream {
            while let Some(length) = bytes[parsed..].iter().position(|byte| *byte == b'\n') {
                if length > 4096 {
                    return Err("CLI_OUTPUT_INVALID");
                }
                let event = serde_json::from_slice(&bytes[parsed..parsed + length])
                    .map_err(|_| "CLI_PROTOCOL_INVALID")?;
                if let Some(update) = stream.accept(&event)? {
                    progress
                        .try_send(update)
                        .map_err(|_| "CLI_OUTPUT_INVALID")?;
                }
                parsed += length + 1;
            }
            if bytes.len() - parsed > 4096 {
                return Err("CLI_OUTPUT_INVALID");
            }
        }
    }
}

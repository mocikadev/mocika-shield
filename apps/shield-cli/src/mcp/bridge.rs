//! 单个 CLI 子进程的有界监督；MCP 不拥有 APK 提交权，也不执行 shell。
use super::policy::{Policy, Result};
use super::protocol::{Progress, Stream};
use std::ffi::OsString;
use std::process::Stdio;
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::process::Command;
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
    let mut command = Command::new(policy.cli());
    command
        .args(&call.arguments)
        .current_dir(policy.working_dir())
        .stdin(if call.controlled {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    command.process_group(0);
    for name in [
        "JAVA_TOOL_OPTIONS",
        "_JAVA_OPTIONS",
        "JDK_JAVA_OPTIONS",
        "RUST_LOG",
    ] {
        command.env_remove(name);
    }
    if !call.host_credentials && (!call.controlled || policy.is_shared()) {
        command
            .env_remove("MOCIKA_SHIELD_KS_PASS")
            .env_remove("MOCIKA_SHIELD_KEY_PASS");
    }
    let mut child = command.spawn().map_err(|_| "CLI_START_FAILED")?;
    let pid = child.id().ok_or("CLI_START_FAILED")? as i32;
    let mut control = child.stdin.take();
    let stdout = child.stdout.take().ok_or("CLI_START_FAILED")?;
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
    let exit = loop {
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
        if let Some(status) = child.try_wait().map_err(|_| "STOP_UNCONFIRMED")? {
            break status;
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
                drop(control.take());
                if started.elapsed() > Duration::from_secs(15) {
                    let _ = child.start_kill();
                    let _ = tokio::time::timeout(Duration::from_secs(2), child.wait()).await;
                    reader.abort();
                    return Err("STOP_UNCONFIRMED");
                }
            } else {
                signal_group(
                    pid,
                    if started.elapsed() < Duration::from_millis(500) {
                        libc::SIGTERM
                    } else {
                        libc::SIGKILL
                    },
                );
                if started.elapsed() > Duration::from_secs(3) {
                    reader.abort();
                    return Err("STOP_UNCONFIRMED");
                }
            }
        }
        sleep(Duration::from_millis(10)).await;
    };
    drop(control);
    // 非受控检查没有工作组租约，由本层确认并回收本次独立进程组。
    if !call.controlled && !finish_group(pid).await {
        reader.abort();
        return Err("STOP_UNCONFIRMED");
    }
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

fn signal_group(pid: i32, signal: i32) {
    if pid > 1 {
        // 只操作由本次 spawn 创建的独立进程组，不接受客户端传入 PID。
        unsafe {
            libc::kill(-pid, signal);
        }
    }
}
async fn finish_group(pid: i32) -> bool {
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        let result = unsafe { libc::kill(-pid, 0) };
        if result == -1 && std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH) {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        signal_group(pid, libc::SIGKILL);
        sleep(Duration::from_millis(20)).await;
    }
}

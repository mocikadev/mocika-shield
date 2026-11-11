//! 受控进程的私有边界：共用租约协议，平台模块只拥有原生资源与停止证明。
use super::worker_protocol::{FrameReader, FrameWriter, Message, Peer};
use std::{
    io,
    sync::mpsc::{self, Receiver},
    time::Duration,
};

#[cfg(unix)]
#[path = "native_group/unix.rs"]
mod platform;
#[cfg(windows)]
#[path = "native_group/windows.rs"]
mod platform;
#[cfg(target_os = "macos")]
#[path = "native_group/macos.rs"]
mod query;
#[cfg(target_os = "linux")]
#[path = "native_group/linux.rs"]
mod query;

pub(crate) use platform::ManagedGroup;
#[cfg(any(test, feature = "mcp-server"))]
mod command;
#[cfg(any(test, feature = "mcp-server"))]
pub(crate) use command::CommandSpec;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ProcessError {
    InvalidGroup,
    InvalidDeadline,
    Transport,
    PermissionDenied,
    Unconfirmed,
    Closed,
}

fn os_error(error: io::Error) -> ProcessError {
    if error.kind() == io::ErrorKind::PermissionDenied {
        ProcessError::PermissionDenied
    } else {
        ProcessError::Unconfirmed
    }
}

/// 仅内部工作入口安装，唯一读线程负责一次开始和租约失效，业务线程不能接管控制流。
pub(super) struct WorkerLease {
    start: Receiver<[u8; 32]>,
}

impl WorkerLease {
    pub fn install(task_id: [u8; 16]) -> Result<Self, ProcessError> {
        let (input, mut guard) = platform::control()?;
        let (sender, start) = mpsc::sync_channel(1);
        std::thread::Builder::new()
            .name("任务租约".into())
            .spawn(move || {
                let mut reader = FrameReader::new(input, task_id, Peer::Parent);
                let mut started = false;
                loop {
                    match reader.read() {
                        Ok(Message::Start { plan_digest }) if !started => {
                            started = true;
                            if guard.begin().is_ok() && sender.try_send(plan_digest).is_ok() {
                                continue;
                            }
                        }
                        _ => {}
                    }
                    // EOF、停止消息、重复开始及畸形帧均撤销本次工作，不把 EOF 当成功。
                    guard.stop();
                }
            })
            .map_err(os_error)?;
        Ok(Self { start })
    }

    pub fn wait_start(&self, timeout: Duration) -> Result<[u8; 32], ProcessError> {
        self.start
            .recv_timeout(timeout)
            .map_err(|_| ProcessError::Unconfirmed)
    }
}

#[cfg(test)]
#[path = "native_group_tests.rs"]
pub(super) mod tests;

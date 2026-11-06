//! macOS 候选进程组控制。只管理自己创建且未回收组长的组，不按进程名终止任务。
use super::worker_protocol::{FrameReader, FrameWriter, Message, Peer};
use rustix::process::{self, Pid, Signal, WaitId, WaitIdOptions};
use std::fs::File;
use std::io;
use std::os::unix::process::CommandExt;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

mod macos;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ProcessError {
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

pub(super) struct ManagedGroup {
    child: Child,
    pid: Pid,
    control: Option<FrameWriter<ChildStdin>>,
    stopped: bool,
    blocked: bool,
}

impl ManagedGroup {
    pub fn spawn(mut command: Command, task_id: [u8; 16]) -> Result<Self, ProcessError> {
        command
            .process_group(0)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        let mut child = command.spawn().map_err(os_error)?;
        let pid = Pid::from_raw(child.id() as i32).ok_or(ProcessError::InvalidGroup)?;
        if process::getpgid(Some(pid)).ok() != Some(pid) {
            // 此时未授权开始，只处理本次直属子进程，绝不对未知进程组发信号。
            let _ = child.kill();
            let _ = child.wait();
            return Err(ProcessError::InvalidGroup);
        }
        let Some(stdin) = child.stdin.take() else {
            let _ = child.kill();
            let _ = child.wait();
            return Err(ProcessError::Transport);
        };
        Ok(Self {
            child,
            pid,
            control: Some(FrameWriter::new(stdin, task_id, Peer::Parent)),
            stopped: false,
            blocked: false,
        })
    }

    pub fn id(&self) -> u32 {
        self.pid.as_raw_nonzero().get() as u32
    }

    pub fn take_stdout(&mut self) -> Option<ChildStdout> {
        self.child.stdout.take()
    }

    pub fn start(&mut self, plan_digest: [u8; 32]) -> Result<(), ProcessError> {
        if self.stopped || self.blocked {
            return Err(ProcessError::Closed);
        }
        self.control
            .as_mut()
            .ok_or(ProcessError::Closed)?
            .send(Message::Start { plan_digest })
            .map_err(|_| ProcessError::Transport)
    }

    /// 取消、超时及正常收尾共用整组停止。租约关闭不能代替这里的原生停止证明。
    pub fn stop(&mut self, grace: Duration) -> Result<(), ProcessError> {
        if self.stopped {
            return Ok(());
        }
        if self.blocked {
            return Err(ProcessError::Closed);
        }
        let deadline = Instant::now()
            .checked_add(grace)
            .ok_or(ProcessError::InvalidDeadline)?;
        let result = self.stop_until(deadline);
        self.control.take();
        if result.is_err() {
            self.blocked = true;
        }
        result
    }

    fn stop_until(&mut self, deadline: Instant) -> Result<(), ProcessError> {
        // 不提前 try_wait/wait。未回收的组长保留 PID，防止后续按复用的组号误杀。
        if !self.quiescent()? {
            if let Err(error) = process::kill_process_group(self.pid, Signal::KILL) {
                if error != rustix::io::Errno::SRCH {
                    return Err(os_error(error.into()));
                }
            }
        }
        self.control.take();
        loop {
            if self.quiescent()? {
                self.child.wait().map_err(os_error)?;
                self.stopped = true;
                return Ok(());
            }
            if Instant::now() >= deadline {
                return Err(ProcessError::Unconfirmed);
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    fn quiescent(&self) -> Result<bool, ProcessError> {
        let exited = self.leader_exited()?;
        Ok(exited && macos::group_quiescent(self.id() as i32)?)
    }

    pub fn leader_exited(&self) -> Result<bool, ProcessError> {
        if self.stopped {
            return Ok(true);
        }
        let exited = process::waitid(
            WaitId::Pid(self.pid),
            WaitIdOptions::EXITED | WaitIdOptions::NOHANG | WaitIdOptions::NOWAIT,
        )
        .map_err(|error| os_error(error.into()))?
        .is_some();
        Ok(exited)
    }
}

impl Drop for ManagedGroup {
    fn drop(&mut self) {
        if !self.stopped && !self.blocked {
            let _ = self.stop(Duration::from_millis(250));
        }
        // 未确认时不回收组长、不重试权限错误；关闭租约只是额外兜底，不是假停止证明。
    }
}

/// 只在隔离的内部工作入口安装。唯一读控制流的线程同时承担父进程存活租约。
pub(super) struct WorkerLease {
    start: Receiver<[u8; 32]>,
}

impl WorkerLease {
    pub fn install(task_id: [u8; 16]) -> Result<Self, ProcessError> {
        let pid = process::getpid();
        if process::getpgrp() != pid {
            return Err(ProcessError::InvalidGroup);
        }
        let lease = rustix::io::fcntl_dupfd_cloexec(std::io::stdin(), 3)
            .map_err(|error| os_error(error.into()))?;
        let null = File::open("/dev/null").map_err(os_error)?;
        // Java 及其后代只继承空标准输入；租约副本为 CLOEXEC，不能被后代消费或延命。
        rustix::stdio::dup2_stdin(null).map_err(|error| os_error(error.into()))?;
        let (sender, start) = mpsc::sync_channel(1);
        std::thread::Builder::new()
            .name("任务租约".into())
            .spawn(move || {
                let mut reader = FrameReader::new(File::from(lease), task_id, Peer::Parent);
                let mut started = false;
                loop {
                    match reader.read() {
                        Ok(Message::Start { plan_digest }) if !started => {
                            started = true;
                            if sender.try_send(plan_digest).is_ok() {
                                continue;
                            }
                        }
                        _ => {}
                    }
                    // EOF、停止消息、重复开始和任何畸形帧都停止本组，绝不执行任意输入。
                    let _ = process::kill_process_group(pid, Signal::KILL);
                    std::process::exit(1);
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
mod tests;

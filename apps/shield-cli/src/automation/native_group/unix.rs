//! Unix 受控进程组。只管理自己创建且未回收组长的组，不按进程名终止任务。
use super::{os_error, query, FrameWriter, Message, Peer, ProcessError};
use rustix::process::{self, Pid, Signal, WaitId, WaitIdOptions};
use std::fs::File;
use std::os::unix::process::CommandExt;
use std::process::{Child, ChildStdin, ChildStdout, Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

pub(crate) struct ManagedGroup {
    pub(super) child: Child,
    pid: Pid,
    pub(super) control: Option<FrameWriter<ChildStdin>>,
    stopped: bool,
    blocked: bool,
    exit: Option<ExitStatus>,
    ordinary: bool,
}

impl ManagedGroup {
    #[cfg(any(test, feature = "mcp-server"))]
    pub fn spawn_command(spec: super::CommandSpec) -> Result<Self, ProcessError> {
        let mut group = Self::spawn(spec.into_command(), [0; 16])?;
        group.ordinary = true;
        Ok(group)
    }
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
            exit: None,
            ordinary: false,
        })
    }

    pub fn id(&self) -> u32 {
        self.pid.as_raw_nonzero().get() as u32
    }

    pub fn take_stdout(&mut self) -> Option<ChildStdout> {
        self.child.stdout.take()
    }

    /// MCP 关闭标准输入请求受控父端取消，不通过发送私有工作帧冒充公共取消。
    #[cfg(any(test, feature = "mcp-server"))]
    pub fn close_input(&mut self) {
        self.control.take();
    }

    #[cfg(any(test, feature = "mcp-server"))]
    pub fn exit_status(&self) -> Option<ExitStatus> {
        self.exit
    }

    pub fn start(&mut self, plan_digest: [u8; 32]) -> Result<(), ProcessError> {
        if self.stopped || self.blocked || self.ordinary {
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
        if !self.quiescent(deadline)? {
            if let Err(error) = process::kill_process_group(self.pid, Signal::KILL) {
                if error != rustix::io::Errno::SRCH {
                    return Err(os_error(error.into()));
                }
            }
        }
        self.control.take();
        loop {
            if self.quiescent(deadline)? {
                self.exit = Some(self.child.wait().map_err(os_error)?);
                self.stopped = true;
                return Ok(());
            }
            if Instant::now() >= deadline {
                return Err(ProcessError::Unconfirmed);
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    fn quiescent(&self, deadline: Instant) -> Result<bool, ProcessError> {
        let exited = self.leader_exited()?;
        Ok(exited && query::group_quiescent(self.id() as i32, deadline)?)
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

pub(super) struct LeaseGuard {
    pid: Pid,
}

pub(super) fn control() -> Result<(File, LeaseGuard), ProcessError> {
    let pid = process::getpid();
    if process::getpgrp() != pid {
        return Err(ProcessError::InvalidGroup);
    }
    let lease = rustix::io::fcntl_dupfd_cloexec(std::io::stdin(), 3)
        .map_err(|error| os_error(error.into()))?;
    let null = File::open("/dev/null").map_err(os_error)?;
    // Java 及其后代只继承空标准输入；租约副本为 CLOEXEC，不能被后代消费或延命。
    rustix::stdio::dup2_stdin(null).map_err(|error| os_error(error.into()))?;
    Ok((File::from(lease), LeaseGuard { pid }))
}

impl LeaseGuard {
    pub(super) fn begin(&mut self) -> Result<(), ProcessError> {
        if process::getpgrp() != self.pid {
            return Err(ProcessError::InvalidGroup);
        }
        Ok(())
    }

    pub(super) fn stop(self) -> ! {
        let _ = process::kill_process_group(self.pid, Signal::KILL);
        std::process::exit(1);
    }
}

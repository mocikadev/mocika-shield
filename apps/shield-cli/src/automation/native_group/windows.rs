//! Windows 本次 Job 监督；协作工作等待开始帧，普通命令创建时归属，收尾证明共用。
use super::{os_error, FrameWriter, Message, Peer, ProcessError};
use crate::automation::windows_job::Job;
use std::{
    fs::File,
    io,
    os::windows::{
        io::{AsRawHandle, FromRawHandle, OwnedHandle},
        process::CommandExt,
    },
    process::{Command, ExitStatus, Stdio},
    time::{Duration, Instant},
};
use windows_sys::Win32::{
    Foundation::{
        DuplicateHandle, DUPLICATE_SAME_ACCESS, INVALID_HANDLE_VALUE, WAIT_OBJECT_0, WAIT_TIMEOUT,
    },
    System::{
        Console::{GetStdHandle, SetStdHandle, STD_INPUT_HANDLE},
        Threading::{GetCurrentProcess, WaitForSingleObject, CREATE_NO_WINDOW},
    },
};

#[path = "windows/parameters.rs"]
#[cfg(any(test, feature = "mcp-server"))]
mod parameters;
#[path = "windows/process.rs"]
mod process;
#[path = "windows/startup.rs"]
#[cfg(any(test, feature = "mcp-server"))]
mod startup;
use process::Process;

pub(crate) struct ManagedGroup {
    pub(super) child: Process,
    pub(super) control: Option<FrameWriter<File>>,
    stdout: Option<File>,
    ordinary: bool,
    job: Job,
    stopped: bool,
    blocked: bool,
    exit: Option<ExitStatus>,
}

impl ManagedGroup {
    /// 仅接入协作工作进程：工作端必须等待私有开始帧，不能用于启动即运行的任意命令。
    pub fn spawn(mut command: Command, task_id: [u8; 16]) -> Result<Self, ProcessError> {
        let job = Job::new().map_err(os_error)?;
        command
            .creation_flags(CREATE_NO_WINDOW)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        let mut child = command.spawn().map_err(os_error)?;
        if let Err(error) = job.assign(&child) {
            // 未发送开始帧，没有业务工具；只处理自己创建的直属子进程及已拥有的 Job。
            let _ = child.kill();
            let _ = child.wait();
            return Err(os_error(error));
        }
        let Some(input) = child.stdin.take() else {
            let _ = job.stop(Instant::now() + Duration::from_millis(250));
            return Err(ProcessError::Transport);
        };
        let stdout = child
            .stdout
            .take()
            .map(|output| File::from(OwnedHandle::from(output)));
        let input = File::from(OwnedHandle::from(input));
        Ok(Self {
            child: Process::from_child(child),
            control: Some(FrameWriter::new(input, task_id, Peer::Parent)),
            stdout,
            ordinary: false,
            job,
            stopped: false,
            blocked: false,
            exit: None,
        })
    }

    /// 普通命令由内核在创建时加入本次 Job，没有先运行再归属的窗口。
    #[cfg(any(test, feature = "mcp-server"))]
    pub fn spawn_command(spec: super::CommandSpec) -> Result<Self, ProcessError> {
        let mut parameters = parameters::Parameters::new(&spec).map_err(os_error)?;
        let job = Job::new().map_err(os_error)?;
        let startup = startup::Startup::new().map_err(os_error)?;
        let child =
            Process::from_information(startup.create(&job, &mut parameters).map_err(os_error)?);
        let mut group = Self {
            child,
            control: Some(FrameWriter::new(startup.input, [0; 16], Peer::Parent)),
            stdout: Some(startup.output),
            ordinary: true,
            job,
            stopped: false,
            blocked: false,
            exit: None,
        };
        if !group.job.contains(&group.child).map_err(os_error)? {
            let _ = group.stop(Duration::from_secs(2));
            return Err(ProcessError::InvalidGroup);
        }
        Ok(group)
    }

    #[cfg(test)]
    pub fn id(&self) -> u32 {
        self.child.id()
    }
    pub fn take_stdout(&mut self) -> Option<File> {
        self.stdout.take()
    }

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
        if !self.job.contains(&self.child).map_err(os_error)? {
            return Err(ProcessError::InvalidGroup);
        }
        self.control
            .as_mut()
            .ok_or(ProcessError::Closed)?
            .send(Message::Start { plan_digest })
            .map_err(|_| ProcessError::Transport)
    }

    pub fn leader_exited(&self) -> Result<bool, ProcessError> {
        // 查询自己仍持有的内核句柄，不按 PID 寻找或终止外部进程。
        match unsafe { WaitForSingleObject(self.child.as_raw_handle(), 0) } {
            WAIT_OBJECT_0 => Ok(true),
            WAIT_TIMEOUT => Ok(false),
            _ => Err(os_error(io::Error::last_os_error())),
        }
    }

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
        self.job.stop(deadline).map_err(os_error)?;
        // Job 计数归零与进程句柄收到退出信号不是同一瞬间，两项都在原期限内确认。
        while !self.leader_exited()? {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(ProcessError::Unconfirmed);
            }
            std::thread::sleep(Duration::from_millis(5).min(remaining));
        }
        self.exit = Some(self.child.wait().map_err(os_error)?);
        self.stopped = true;
        Ok(())
    }
}

impl Drop for ManagedGroup {
    fn drop(&mut self) {
        if !self.stopped && !self.blocked {
            let _ = self.stop(Duration::from_millis(250));
        }
        // 外层 Job 句柄只在父端；关闭仍会兜底终止，但不能改写未确认的任务结果。
    }
}

pub(super) struct LeaseGuard {
    _stdin: File,
    job: Option<Job>,
}

pub(super) fn control() -> Result<(File, LeaseGuard), ProcessError> {
    // 仅内部工作入口调用，尚未读取或借出标准输入；私有副本不允许后代继承。
    let original = unsafe { GetStdHandle(STD_INPUT_HANDLE) };
    if original.is_null() || original == INVALID_HANDLE_VALUE {
        return Err(ProcessError::Transport);
    }
    let current = unsafe { GetCurrentProcess() };
    let mut copied = std::ptr::null_mut();
    if unsafe {
        DuplicateHandle(
            current,
            original,
            current,
            &mut copied,
            0,
            0,
            DUPLICATE_SAME_ACCESS,
        )
    } == 0
    {
        return Err(os_error(io::Error::last_os_error()));
    }
    // 安全：复制成功，句柄唯一归 File 所有；原标准输入只在替换成功后关闭。
    let input = File::from(unsafe { OwnedHandle::from_raw_handle(copied) });
    let null = File::open("NUL").map_err(os_error)?;
    if unsafe { SetStdHandle(STD_INPUT_HANDLE, null.as_raw_handle()) } == 0 {
        return Err(os_error(io::Error::last_os_error()));
    }
    drop(unsafe { OwnedHandle::from_raw_handle(original) });
    Ok((
        input,
        LeaseGuard {
            _stdin: null,
            job: None,
        },
    ))
}

impl LeaseGuard {
    pub(super) fn begin(&mut self) -> Result<(), ProcessError> {
        // 父端已确认外层归属后才发送开始；此时再创建内层 Job，固定合法嵌套顺序。
        // 不复制父端 Job 句柄，父死时外层唯一句柄仍能触发内核清场。
        let job = Job::new().map_err(os_error)?;
        job.assign_current().map_err(os_error)?;
        self.job = Some(job);
        Ok(())
    }

    pub(super) fn stop(self) -> ! {
        // 内层句柄只在本读线程；租约失效即关闭，停止自己与所有业务后代。
        // 对外的停止证明仍由父端查询外层 Job，而非本线程自行宣称。
        drop(self.job);
        std::process::exit(1);
    }
}

#[cfg(test)]
pub(super) fn process_running(pid: i32, _group: i32) -> Result<bool, ProcessError> {
    use windows_sys::Win32::{
        Foundation::ERROR_INVALID_PARAMETER,
        System::Threading::{OpenProcess, PROCESS_SYNCHRONIZE},
    };
    let pid = u32::try_from(pid).map_err(|_| ProcessError::InvalidGroup)?;
    // 只读测试夹具记录的 PID，仅等待权限；不作为产品的停止证明或终止入口。
    let raw = unsafe { OpenProcess(PROCESS_SYNCHRONIZE, 0, pid) };
    if raw.is_null() {
        let error = io::Error::last_os_error();
        return if error.raw_os_error() == Some(ERROR_INVALID_PARAMETER as i32) {
            Ok(false)
        } else {
            Err(os_error(error))
        };
    }
    let handle = unsafe { OwnedHandle::from_raw_handle(raw) };
    match unsafe { WaitForSingleObject(handle.as_raw_handle(), 0) } {
        WAIT_OBJECT_0 => Ok(false),
        WAIT_TIMEOUT => Ok(true),
        _ => Err(os_error(io::Error::last_os_error())),
    }
}

#[cfg(test)]
#[path = "windows/tests.rs"]
mod tests;

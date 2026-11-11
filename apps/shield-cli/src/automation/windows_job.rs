//! Windows Job 的私有所有权与停止核对，不负责启动命令、读取凭据或提交 APK。
use std::{
    io,
    mem::size_of,
    os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle},
    time::{Duration, Instant},
};
use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, IsProcessInJob,
    JobObjectBasicAccountingInformation, JobObjectExtendedLimitInformation,
    QueryInformationJobObject, SetInformationJobObject, TerminateJobObject,
    JOBOBJECT_BASIC_ACCOUNTING_INFORMATION, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
    JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
};
use windows_sys::Win32::System::Threading::GetCurrentProcess;

pub(super) struct Job {
    handle: OwnedHandle,
}

impl AsRawHandle for Job {
    fn as_raw_handle(&self) -> std::os::windows::io::RawHandle {
        self.handle.as_raw_handle()
    }
}

impl Job {
    pub(super) fn new() -> io::Result<Self> {
        // 安全：不使用名称或可继承属性；成功后唯一句柄立即交给 RAII 所有者。
        let raw = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
        if raw.is_null() {
            return Err(io::Error::last_os_error());
        }
        let job = Self {
            handle: unsafe { OwnedHandle::from_raw_handle(raw) },
        };
        // 安全：结构为 SDK 完整大小，零值没有额外限制；只设置关闭时整组终止。
        let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        let ok = unsafe {
            SetInformationJobObject(
                job.handle.as_raw_handle(),
                JobObjectExtendedLimitInformation,
                (&info as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            )
        };
        if ok == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(job)
    }

    /// 调用方必须先建立工作开始门槛；分配及归属确认前不得让工作进程执行工具。
    pub(super) fn assign(&self, process: &impl AsRawHandle) -> io::Result<()> {
        // 安全：借用的进程与本次 Job 句柄在调用期间均存活，不接收外部 PID。
        if unsafe { AssignProcessToJobObject(self.handle.as_raw_handle(), process.as_raw_handle()) }
            == 0
        {
            return Err(io::Error::last_os_error());
        }
        if !self.contains(process)? {
            return Err(io::Error::other("不能确认本次 Job 归属"));
        }
        Ok(())
    }

    /// 工作端只把自己加入私有嵌套 Job，不接收或关闭当前进程的伪句柄。
    pub(super) fn assign_current(&self) -> io::Result<()> {
        let current = unsafe { GetCurrentProcess() };
        if unsafe { AssignProcessToJobObject(self.handle.as_raw_handle(), current) } == 0 {
            return Err(io::Error::last_os_error());
        }
        let mut member = 0;
        if unsafe { IsProcessInJob(current, self.handle.as_raw_handle(), &mut member) } == 0 {
            return Err(io::Error::last_os_error());
        }
        if member == 0 {
            return Err(io::Error::other("不能确认工作端私有 Job 归属"));
        }
        Ok(())
    }

    pub(super) fn contains(&self, process: &impl AsRawHandle) -> io::Result<bool> {
        let mut member = 0;
        // 安全：输出为已初始化 BOOL，查询明确 Job，不把其他父 Job 的归属当成本次归属。
        if unsafe {
            IsProcessInJob(
                process.as_raw_handle(),
                self.handle.as_raw_handle(),
                &mut member,
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(member != 0)
    }

    pub(super) fn active(&self) -> io::Result<u32> {
        let mut info = JOBOBJECT_BASIC_ACCOUNTING_INFORMATION::default();
        let mut written = 0;
        // 安全：缓冲区对齐和大小对应 SDK 类型；短读不能作为停止证明。
        if unsafe {
            QueryInformationJobObject(
                self.handle.as_raw_handle(),
                JobObjectBasicAccountingInformation,
                (&mut info as *mut JOBOBJECT_BASIC_ACCOUNTING_INFORMATION).cast(),
                size_of::<JOBOBJECT_BASIC_ACCOUNTING_INFORMATION>() as u32,
                &mut written,
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        if written as usize != size_of::<JOBOBJECT_BASIC_ACCOUNTING_INFORMATION>() {
            return Err(io::Error::other("本次 Job 状态不完整"));
        }
        Ok(info.ActiveProcesses)
    }

    /// 只有完整查询确认活跃数为零才返回成功；关闭句柄仅是父死兜底，不是假停止回执。
    pub(super) fn stop(&self, deadline: Instant) -> io::Result<()> {
        if self.active()? != 0 {
            // 安全：只终止本对象拥有的 Job，不按进程名、全局枚举或其他 Job 清场。
            if unsafe { TerminateJobObject(self.handle.as_raw_handle(), 1) } == 0 {
                return Err(io::Error::last_os_error());
            }
        }
        loop {
            if self.active()? == 0 {
                return Ok(());
            }
            if Instant::now() >= deadline {
                return Err(io::ErrorKind::TimedOut.into());
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }
}

#[cfg(test)]
#[path = "windows_job_tests.rs"]
mod tests;

//! Windows 直属进程的内核句柄；退出码只在确认句柄已退出后读取，不用 PID 清场。
#[cfg(any(test, feature = "mcp-server"))]
use std::os::windows::io::FromRawHandle;
use std::{
    io,
    os::windows::{
        io::{AsRawHandle, OwnedHandle, RawHandle},
        process::ExitStatusExt,
    },
    process::{Child, ExitStatus},
};
#[cfg(test)]
use windows_sys::Win32::System::Threading::TerminateProcess;
#[cfg(any(test, feature = "mcp-server"))]
use windows_sys::Win32::System::Threading::PROCESS_INFORMATION;
use windows_sys::Win32::{
    Foundation::{WAIT_OBJECT_0, WAIT_TIMEOUT},
    System::Threading::{GetExitCodeProcess, WaitForSingleObject},
};

pub(in crate::automation::native_group) struct Process {
    handle: OwnedHandle,
    #[cfg(test)]
    id: u32,
}
impl Process {
    pub(super) fn from_child(child: Child) -> Self {
        #[cfg(test)]
        let id = child.id();
        Self {
            handle: child.into(),
            #[cfg(test)]
            id,
        }
    }
    #[cfg(any(test, feature = "mcp-server"))]
    pub(super) fn from_information(info: PROCESS_INFORMATION) -> Self {
        // 安全：仅在 CreateProcess 成功后消费两个唯一句柄；线程句柄立即关闭。
        drop(unsafe { OwnedHandle::from_raw_handle(info.hThread) });
        Self {
            handle: unsafe { OwnedHandle::from_raw_handle(info.hProcess) },
            #[cfg(test)]
            id: info.dwProcessId,
        }
    }
    #[cfg(test)]
    pub fn id(&self) -> u32 {
        self.id
    }
    pub fn exited(&self) -> io::Result<bool> {
        match unsafe { WaitForSingleObject(self.as_raw_handle(), 0) } {
            WAIT_OBJECT_0 => Ok(true),
            WAIT_TIMEOUT => Ok(false),
            _ => Err(io::Error::last_os_error()),
        }
    }
    pub fn wait(&mut self) -> io::Result<ExitStatus> {
        if !self.exited()? {
            return Err(io::ErrorKind::WouldBlock.into());
        }
        let mut code = 0;
        if unsafe { GetExitCodeProcess(self.as_raw_handle(), &mut code) } == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(ExitStatus::from_raw(code))
    }
    #[cfg(test)]
    pub fn kill(&mut self) -> io::Result<()> {
        if unsafe { TerminateProcess(self.as_raw_handle(), 1) } == 0 && !self.exited()? {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }
}
impl AsRawHandle for Process {
    fn as_raw_handle(&self) -> RawHandle {
        self.handle.as_raw_handle()
    }
}

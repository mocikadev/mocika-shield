//! macOS 原生只读状态核对；结构及常量依据本机 SDK 和已锁定 libc，拒绝短读/超限。
use super::{os_error, ProcessError};
use std::io;

pub(super) fn process_running(pid: i32, group: i32) -> Result<bool, ProcessError> {
    let mut info = std::mem::MaybeUninit::<libc::proc_bsdinfo>::zeroed();
    let size = std::mem::size_of::<libc::proc_bsdinfo>();
    // 安全：缓冲区大小与 libc/SDK 的结构一致，完整返回后才读取内容。
    let count = unsafe {
        *libc::__error() = 0;
        libc::proc_pidinfo(
            pid,
            libc::PROC_PIDTBSDINFO,
            0,
            info.as_mut_ptr().cast(),
            size as i32,
        )
    };
    if count as usize != size {
        let error = io::Error::last_os_error();
        if error.raw_os_error() == Some(libc::ESRCH) {
            return Ok(false);
        }
        return Err(os_error(error));
    }
    // 安全：已确认系统返回整个结构。
    let info = unsafe { info.assume_init() };
    Ok(info.pbi_pgid == group as u32 && info.pbi_status != libc::SZOMB)
}

pub(super) fn group_quiescent(
    group: i32,
    deadline: std::time::Instant,
) -> Result<bool, ProcessError> {
    let mut pids = [0i32; 4096];
    let capacity = std::mem::size_of_val(&pids);
    // 安全：传入已初始化的有界 PID 数组，不让系统报告值驱动无界分配。
    let count = unsafe {
        *libc::__error() = 0;
        libc::proc_listpgrppids(group, pids.as_mut_ptr().cast(), capacity as i32)
    };
    // proc_listpgrppids 返回 PID 个数，而不是底层 proc_listpids 的字节数。
    if count < 0 || count as usize >= pids.len() {
        return Err(ProcessError::Unconfirmed);
    }
    if count == 0 {
        let error = io::Error::last_os_error();
        if error.raw_os_error() != Some(0) && error.raw_os_error() != Some(libc::ESRCH) {
            return Err(os_error(error));
        }
    }
    for &pid in &pids[..count as usize] {
        if std::time::Instant::now() >= deadline {
            return Err(ProcessError::Unconfirmed);
        }
        // 调用方已用 WNOWAIT 得到组长退出的肯定证明，不向退出中的组长查询进程详情。
        if pid == group {
            continue;
        }
        if pid > 0 && process_running(pid, group)? {
            return Ok(false);
        }
    }
    Ok(true)
}

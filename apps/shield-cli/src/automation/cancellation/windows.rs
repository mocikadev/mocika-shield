//! Windows 只登记可持续处理的 Ctrl+C／Break；强制关闭仍由 Job 句柄关闭兜底。
use std::sync::atomic::{AtomicBool, Ordering};
use windows_sys::Win32::{
    Foundation::ERROR_INVALID_HANDLE,
    System::Console::{SetConsoleCtrlHandler, CTRL_BREAK_EVENT, CTRL_C_EVENT},
};
static SIGNAL: AtomicBool = AtomicBool::new(false);
static INSTALLED: AtomicBool = AtomicBool::new(false);

unsafe extern "system" fn handler(event: u32) -> i32 {
    if event == CTRL_C_EVENT || event == CTRL_BREAK_EVENT {
        SIGNAL.store(true, Ordering::SeqCst);
        1
    } else {
        0
    }
}
pub(super) struct Signals {
    active: bool,
}
impl Signals {
    pub(super) fn install() -> Result<Self, ()> {
        INSTALLED
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .map_err(|_| ())?;
        SIGNAL.store(false, Ordering::SeqCst);
        if unsafe { SetConsoleCtrlHandler(Some(handler), 1) } != 0 {
            return Ok(Self { active: true });
        }
        // 无控制台的 Agent 调用仍可通过专用 stdin／输出错误取消，不分配控制台。
        if std::io::Error::last_os_error().raw_os_error() == Some(ERROR_INVALID_HANDLE as i32) {
            return Ok(Self { active: false });
        }
        INSTALLED.store(false, Ordering::SeqCst);
        Err(())
    }
}
impl Drop for Signals {
    fn drop(&mut self) {
        if self.active {
            unsafe {
                SetConsoleCtrlHandler(Some(handler), 0);
            }
        }
        INSTALLED.store(false, Ordering::SeqCst);
    }
}
pub(super) fn signal_received() -> bool {
    SIGNAL.load(Ordering::SeqCst)
}
pub(super) fn output_closed() -> bool {
    // 普通写线程的失败通知与有限收尾共用 Cancel。不能在只写句柄上用 PeekNamedPipe，
    // 也不为模拟 Unix poll 在监督线程执行可能等待同步写完成的管道查询。
    false
}

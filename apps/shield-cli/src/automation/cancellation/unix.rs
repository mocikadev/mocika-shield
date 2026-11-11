//! 独立 CLI 的 Unix 信号登记与无阻塞输出状态查询，不执行清场。
use std::sync::atomic::{AtomicI32, Ordering};
static SIGNAL: AtomicI32 = AtomicI32::new(0);

extern "C" fn handler(signal: libc::c_int) {
    let _ = SIGNAL.compare_exchange(0, signal, Ordering::Relaxed, Ordering::Relaxed);
}
pub(super) struct Signals {
    old: [(libc::c_int, libc::sigaction); 2],
}
impl Signals {
    pub(super) fn install() -> Result<Self, ()> {
        unsafe {
            let mut action: libc::sigaction = std::mem::zeroed();
            action.sa_sigaction = handler as *const () as usize;
            libc::sigemptyset(&mut action.sa_mask);
            let mut first = std::mem::zeroed();
            let mut second = std::mem::zeroed();
            SIGNAL.store(0, Ordering::SeqCst);
            if libc::sigaction(libc::SIGINT, &action, &mut first) != 0 {
                return Err(());
            }
            if libc::sigaction(libc::SIGTERM, &action, &mut second) != 0 {
                libc::sigaction(libc::SIGINT, &first, std::ptr::null_mut());
                return Err(());
            }
            Ok(Self {
                old: [(libc::SIGINT, first), (libc::SIGTERM, second)],
            })
        }
    }
}
impl Drop for Signals {
    fn drop(&mut self) {
        for (signal, action) in &self.old {
            unsafe {
                libc::sigaction(*signal, action, std::ptr::null_mut());
            }
        }
    }
}
pub(super) fn signal_received() -> bool {
    SIGNAL.load(Ordering::Relaxed) != 0
}
pub(super) fn output_closed() -> bool {
    let mut descriptor = libc::pollfd {
        fd: libc::STDOUT_FILENO,
        events: 0,
        revents: 0,
    };
    unsafe {
        libc::poll(&mut descriptor, 1, 0) > 0
            && descriptor.revents & (libc::POLLHUP | libc::POLLERR | libc::POLLNVAL) != 0
    }
}

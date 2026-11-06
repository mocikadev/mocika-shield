//! 仅在新命令生命周期内登记取消；信号处理器不清场，stdin 不接受密码或命令。
use super::*;
use std::sync::atomic::{AtomicI32, AtomicU8};
use std::thread::JoinHandle;

static SIGNAL: AtomicI32 = AtomicI32::new(0);

#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub(super) enum Cause {
    None = 0,
    Input = 1,
    Closed = 2,
    Invalid = 3,
    Signal = 4,
    Output = 5,
}

#[derive(Clone)]
pub(super) struct Cancel {
    flag: Arc<AtomicBool>,
    cause: Arc<AtomicU8>,
}
impl Cancel {
    pub fn new() -> Self {
        Self {
            flag: Arc::new(AtomicBool::new(false)),
            cause: Arc::new(AtomicU8::new(0)),
        }
    }
    pub fn request(&self, cause: Cause) {
        let _ = self
            .cause
            .compare_exchange(0, cause as u8, Ordering::SeqCst, Ordering::SeqCst);
        self.flag.store(true, Ordering::SeqCst);
    }
    pub fn cause(&self) -> Cause {
        match self.cause.load(Ordering::SeqCst) {
            1 => Cause::Input,
            2 => Cause::Closed,
            3 => Cause::Invalid,
            4 => Cause::Signal,
            5 => Cause::Output,
            _ => Cause::None,
        }
    }
}

extern "C" fn signal_handler(signal: libc::c_int) {
    let _ = SIGNAL.compare_exchange(0, signal, Ordering::Relaxed, Ordering::Relaxed);
}

struct Signals {
    old: [(libc::c_int, libc::sigaction); 2],
}
impl Signals {
    fn install() -> Result<Self, ()> {
        // macOS 的 sigaction 只在独立 CLI 父进程中安装，恢复先前的处理方式。
        unsafe {
            let mut action: libc::sigaction = std::mem::zeroed();
            action.sa_sigaction = signal_handler as *const () as usize;
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

pub(super) struct Controls {
    cancel: Cancel,
    done: Arc<AtomicBool>,
    watcher: Option<JoinHandle<()>>,
    _signals: Signals,
}
impl Controls {
    pub fn new(input: bool) -> Result<Self, ()> {
        let signals = Signals::install()?;
        let cancel = Cancel::new();
        let done = Arc::new(AtomicBool::new(false));
        let ended = done.clone();
        let watched = cancel.clone();
        let watcher = std::thread::Builder::new()
            .name("自动化取消监督".into())
            .spawn(move || {
                while !ended.load(Ordering::SeqCst) {
                    if SIGNAL.load(Ordering::Relaxed) != 0 {
                        watched.request(Cause::Signal);
                    }
                    let mut descriptor = libc::pollfd {
                        fd: libc::STDOUT_FILENO,
                        events: 0,
                        revents: 0,
                    };
                    let broken = unsafe { libc::poll(&mut descriptor, 1, 0) } > 0
                        && descriptor.revents & (libc::POLLHUP | libc::POLLERR | libc::POLLNVAL)
                            != 0;
                    if broken {
                        watched.request(Cause::Output);
                    }
                    std::thread::sleep(Duration::from_millis(5));
                }
            })
            .map_err(|_| ())?;
        let controls = Self {
            cancel,
            done,
            watcher: Some(watcher),
            _signals: signals,
        };
        if input {
            let receiver = controls.cancel.clone();
            // 独立 CLI 退出会结束仍在等待输入的线程，不向 GUI 或核心安装全局读线程。
            std::thread::Builder::new()
                .name("自动化取消输入".into())
                .spawn(move || read_control(std::io::stdin().lock(), &receiver))
                .map_err(|_| ())?;
        }
        Ok(controls)
    }
    pub fn cancel(&self) -> Arc<AtomicBool> {
        self.cancel.flag.clone()
    }
    pub fn clone_signal(&self) -> Cancel {
        self.cancel.clone()
    }
    pub fn cause(&self) -> Cause {
        self.cancel.cause()
    }
}
impl Drop for Controls {
    fn drop(&mut self) {
        self.done.store(true, Ordering::SeqCst);
        if let Some(watcher) = self.watcher.take() {
            let _ = watcher.join();
        }
    }
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
enum Input {
    Cancel {},
}

fn read_control(mut input: impl Read, cancel: &Cancel) {
    let mut bytes = Vec::new();
    let mut byte = [0u8; 1];
    loop {
        match input.read(&mut byte) {
            Ok(0) => {
                cancel.request(if bytes.is_empty() {
                    Cause::Closed
                } else {
                    Cause::Invalid
                });
                return;
            }
            Ok(_) if byte[0] == b'\n' => {
                cancel.request(if serde_json::from_slice::<Input>(&bytes).is_ok() {
                    Cause::Input
                } else {
                    Cause::Invalid
                });
                return;
            }
            Ok(_) if bytes.len() < 1024 => bytes.push(byte[0]),
            Err(err) if err.kind() == std::io::ErrorKind::Interrupted => continue,
            _ => {
                cancel.request(Cause::Invalid);
                return;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn 取消输入有界且未知字段不能作为有效取消() {
        for (bytes, expected) in [
            (b"{\"type\":\"cancel\"}\n".as_slice(), Cause::Input),
            (
                b"{\"type\":\"cancel\",\"password\":\"x\"}\n".as_slice(),
                Cause::Invalid,
            ),
            (b"".as_slice(), Cause::Closed),
            (b"invalid\n".as_slice(), Cause::Invalid),
        ] {
            let cancel = Cancel {
                flag: Arc::new(AtomicBool::new(false)),
                cause: Arc::new(AtomicU8::new(0)),
            };
            read_control(bytes, &cancel);
            assert!(cancel.flag.load(Ordering::SeqCst));
            assert!(cancel.cause() == expected);
        }
        let cancel = Cancel {
            flag: Arc::new(AtomicBool::new(false)),
            cause: Arc::new(AtomicU8::new(0)),
        };
        read_control(vec![b'x'; 1026].as_slice(), &cancel);
        assert!(cancel.cause() == Cause::Invalid);
    }
}

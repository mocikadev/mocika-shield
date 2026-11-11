//! 仅在新命令生命周期内登记取消；信号处理器不清场，stdin 不接受密码或命令。
use serde::Deserialize;
use std::thread::JoinHandle;
use std::{
    io::Read,
    sync::{
        atomic::{AtomicBool, AtomicU8, Ordering},
        Arc,
    },
    time::Duration,
};
#[cfg(unix)]
#[path = "cancellation/unix.rs"]
mod platform;
#[cfg(windows)]
#[path = "cancellation/windows.rs"]
mod platform;
use platform::Signals;

/// MCP 只借用控制台信号登记，不安装同步 stdin 读线程或第二套业务取消状态。
#[cfg(all(windows, feature = "mcp-server"))]
pub(super) struct SignalWatch {
    _signals: Signals,
}
#[cfg(all(windows, feature = "mcp-server"))]
impl SignalWatch {
    pub fn install() -> Result<Self, ()> {
        Ok(Self {
            _signals: Signals::install()?,
        })
    }
    pub fn interrupted(&self) -> bool {
        platform::signal_received()
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub(crate) enum Cause {
    None = 0,
    Input = 1,
    Closed = 2,
    Invalid = 3,
    Signal = 4,
    Output = 5,
}

#[derive(Clone)]
pub(crate) struct Cancel {
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

pub(crate) struct Controls {
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
                    if platform::signal_received() {
                        watched.request(Cause::Signal);
                    }
                    if platform::output_closed() {
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

#[cfg(test)]
#[path = "cancellation/process_tests.rs"]
mod process_tests;

//! 公共事件字节的有界交付；不解析业务协议，不拥有任务终态或文件提交权。
use super::cancellation::{Cancel, Cause};
use std::{
    io::Write,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{self, SyncSender},
        Arc, Mutex,
    },
    thread::JoinHandle,
    time::{Duration, Instant},
};

struct State {
    writing: Mutex<Option<Instant>>,
    failed: AtomicBool,
    done: AtomicBool,
}
impl State {
    fn fail(&self, cancel: &Cancel) {
        self.failed.store(true, Ordering::SeqCst);
        cancel.request(Cause::Output);
    }
}

pub(super) struct Output {
    sender: Option<SyncSender<Vec<u8>>>,
    writer: Option<JoinHandle<bool>>,
    monitor: Option<JoinHandle<()>>,
    state: Arc<State>,
    cancel: Cancel,
}
impl Output {
    pub fn new(writer: impl Write + Send + 'static, cancel: Cancel) -> Result<Self, ()> {
        Self::with_allowance(writer, cancel, Duration::from_secs(2))
    }
    fn with_allowance(
        mut writer: impl Write + Send + 'static,
        cancel: Cancel,
        allowance: Duration,
    ) -> Result<Self, ()> {
        let (sender, receiver) = mpsc::sync_channel::<Vec<u8>>(32);
        let state = Arc::new(State {
            writing: Mutex::new(None),
            failed: AtomicBool::new(false),
            done: AtomicBool::new(false),
        });
        let writing = state.clone();
        let failed = cancel.clone();
        let thread = std::thread::Builder::new()
            .name("自动化事件输出".into())
            .spawn(move || {
                for bytes in receiver {
                    if writing.failed.load(Ordering::SeqCst) {
                        return false;
                    }
                    *writing.writing.lock().unwrap() = Some(Instant::now());
                    let result = writer.write_all(&bytes).and_then(|_| writer.flush());
                    *writing.writing.lock().unwrap() = None;
                    if result.is_err() {
                        writing.fail(&failed);
                        return false;
                    }
                }
                true
            })
            .map_err(|_| ())?;
        let watched = state.clone();
        let stopped = cancel.clone();
        let monitor = match std::thread::Builder::new()
            .name("自动化输出期限".into())
            .spawn(move || {
                while !watched.done.load(Ordering::SeqCst) {
                    let expired = watched
                        .writing
                        .lock()
                        .unwrap()
                        .is_some_and(|since| since.elapsed() >= allowance);
                    if expired {
                        watched.fail(&stopped);
                        return;
                    }
                    std::thread::sleep(Duration::from_millis(5));
                }
            }) {
            Ok(monitor) => monitor,
            Err(_) => {
                drop(sender);
                let _ = thread.join();
                return Err(());
            }
        };
        Ok(Self {
            sender: Some(sender),
            writer: Some(thread),
            monitor: Some(monitor),
            state,
            cancel,
        })
    }
    pub fn send(&self, bytes: Vec<u8>) -> bool {
        if bytes.len() > 4097
            || self.state.failed.load(Ordering::SeqCst)
            || self
                .sender
                .as_ref()
                .is_none_or(|sender| sender.try_send(bytes).is_err())
        {
            self.state.fail(&self.cancel);
            return false;
        }
        true
    }
    pub fn finish(mut self, grace: Duration) -> bool {
        self.sender.take();
        let Some(deadline) = Instant::now().checked_add(grace) else {
            self.state.fail(&self.cancel);
            return false;
        };
        while !self.writer.as_ref().unwrap().is_finished() {
            if Instant::now() >= deadline {
                self.state.fail(&self.cancel);
                return false;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        let written = self.writer.take().unwrap().join().unwrap_or(false);
        let monitored = self.stop_monitor(deadline);
        written && monitored && !self.state.failed.load(Ordering::SeqCst)
    }
    fn stop_monitor(&mut self, deadline: Instant) -> bool {
        self.state.done.store(true, Ordering::SeqCst);
        if let Some(monitor) = self.monitor.take() {
            while !monitor.is_finished() {
                if Instant::now() >= deadline {
                    return false;
                }
                std::thread::sleep(Duration::from_millis(1));
            }
            return monitor.join().is_ok();
        }
        true
    }
}
impl Drop for Output {
    fn drop(&mut self) {
        self.sender.take();
        let _ = self.stop_monitor(Instant::now() + Duration::from_millis(100));
        // 只在独立 CLI 中使用；阻塞写线程不能阻止清场，进程退出会结束它，不假称已回收。
    }
}

#[cfg(test)]
mod tests;

use super::*;
use std::{io, sync::Condvar};

struct Blocked {
    gate: Arc<(Mutex<bool>, Condvar)>,
    ended: mpsc::Sender<()>,
}
impl Write for Blocked {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let (lock, wake) = &*self.gate;
        let mut ready = lock.lock().unwrap();
        while !*ready {
            ready = wake.wait(ready).unwrap();
        }
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
impl Drop for Blocked {
    fn drop(&mut self) {
        let _ = self.ended.send(());
    }
}

#[test]
fn 阻塞写入在任务尚未收尾时也会按期请求取消() {
    let gate = Arc::new((Mutex::new(false), Condvar::new()));
    let (ended, exit) = mpsc::channel();
    let cancel = Cancel::new();
    let output = Output::with_allowance(
        Blocked {
            gate: gate.clone(),
            ended,
        },
        cancel.clone(),
        Duration::from_millis(30),
    )
    .ok()
    .unwrap();
    assert!(output.send(b"bounded\n".to_vec()));
    let deadline = Instant::now() + Duration::from_secs(2);
    while cancel.cause() == Cause::None {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(cancel.cause() == Cause::Output);
    let started = Instant::now();
    assert!(!output.finish(Duration::from_millis(20)));
    assert!(started.elapsed() < Duration::from_secs(1));
    let (lock, wake) = &*gate;
    *lock.lock().unwrap() = true;
    wake.notify_all();
    exit.recv_timeout(Duration::from_secs(2)).unwrap();
}

struct Broken;
impl Write for Broken {
    fn write(&mut self, _: &[u8]) -> io::Result<usize> {
        Err(io::ErrorKind::BrokenPipe.into())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
#[test]
fn 写入失败或队列拒绝不能因为线程退出就宣称交付成功() {
    let cancel = Cancel::new();
    let output = Output::new(Broken, cancel.clone()).ok().unwrap();
    assert!(output.send(b"fixture\n".to_vec()));
    assert!(!output.finish(Duration::from_secs(1)));
    assert!(cancel.cause() == Cause::Output);
    let output = Output::new(io::sink(), Cancel::new()).ok().unwrap();
    assert!(!output.send(vec![b'x'; 4098]));
    assert!(!output.finish(Duration::from_secs(1)));
}

#[test]
fn 正常交付有限字节且不触发取消() {
    let cancel = Cancel::new();
    let output = Output::new(io::sink(), cancel.clone()).ok().unwrap();
    assert!(output.send(b"fixture\n".to_vec()));
    assert!(output.finish(Duration::from_secs(1)));
    assert!(cancel.cause() == Cause::None);
}

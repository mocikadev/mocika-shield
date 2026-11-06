//! 有界异步读消息，避免阻塞父端期限监督；不判断任务状态，不解析原始工具日志。
use super::{error, CheckError};
use crate::automation::worker_protocol::{FrameReader, Message, Peer, ProtocolError};
use std::io::Read;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc::{self, Receiver},
    Arc,
};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

pub(super) struct Pump {
    receiver: Receiver<Result<Message, ProtocolError>>,
    bad: Arc<AtomicBool>,
    thread: JoinHandle<()>,
}
impl Pump {
    pub fn new(input: impl Read + Send + 'static, id: [u8; 16]) -> Self {
        let (sender, receiver) = mpsc::sync_channel(4);
        let bad = Arc::new(AtomicBool::new(false));
        let bad_reader = bad.clone();
        let thread = std::thread::spawn(move || {
            let mut reader = FrameReader::new(input, id, Peer::Worker);
            loop {
                let event = reader.read();
                let finished = event.is_err();
                // 满队列产生背压，不能把合法的短时进度突发误判为坏协议。
                if sender.send(event).is_err() {
                    bad_reader.store(true, Ordering::SeqCst);
                    break;
                }
                if finished {
                    break;
                }
            }
        });
        Self {
            receiver,
            bad,
            thread,
        }
    }

    pub fn next(&self) -> Result<Option<Message>, CheckError> {
        if self.bad.load(Ordering::SeqCst) {
            return Err(error("PROTOCOL_FAILED"));
        }
        match self.receiver.recv_timeout(Duration::from_millis(20)) {
            Ok(Ok(message)) => Ok(Some(message)),
            Err(mpsc::RecvTimeoutError::Timeout) => Ok(None),
            Ok(Err(ProtocolError::EndOfStream)) => Err(error("WORKER_CHANNEL_CLOSED")),
            _ => {
                self.bad.store(true, Ordering::SeqCst);
                Err(error("PROTOCOL_FAILED"))
            }
        }
    }

    /// 先由调用方停止工作组，再有界回收读线程；边等边排空队列，避免背压导致假死。
    pub fn finish(self) -> Result<bool, ()> {
        let deadline = Instant::now() + Duration::from_millis(250);
        let mut valid = true;
        while !self.thread.is_finished() {
            for event in self.receiver.try_iter() {
                valid &= matches!(event, Err(ProtocolError::EndOfStream));
            }
            if Instant::now() >= deadline {
                return Err(());
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        Ok(valid
            && self.thread.join().is_ok()
            && !self.bad.load(Ordering::SeqCst)
            && self
                .receiver
                .try_iter()
                .all(|event| matches!(event, Err(ProtocolError::EndOfStream))))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::automation::{task_lifecycle::Stage, worker_protocol::FrameWriter};
    use std::io::Cursor;

    #[test]
    fn 短时突发消息通过背压完整交付() {
        let mut bytes = Vec::new();
        let mut writer = FrameWriter::new(&mut bytes, [0; 16], Peer::Worker);
        for _ in 0..64 {
            writer
                .send(Message::Progress {
                    stage: Stage::Prepare,
                })
                .unwrap();
        }
        let pump = Pump::new(Cursor::new(bytes), [0; 16]);
        std::thread::sleep(Duration::from_millis(5));
        for _ in 0..64 {
            assert!(matches!(pump.next(), Ok(Some(Message::Progress { .. }))));
        }
        assert!(matches!(pump.finish(), Ok(true)));
    }

    #[test]
    fn 收尾排空多余消息会拒绝但不会被满队列卡住() {
        let mut bytes = Vec::new();
        let mut writer = FrameWriter::new(&mut bytes, [0; 16], Peer::Worker);
        for _ in 0..64 {
            writer.send(Message::Hello {}).unwrap();
        }
        let pump = Pump::new(Cursor::new(bytes), [0; 16]);
        assert!(matches!(pump.finish(), Ok(false)));
    }
}

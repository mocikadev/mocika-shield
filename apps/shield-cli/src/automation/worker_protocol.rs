//! 有界内部消息编解码；不读任务配置/密码，不把 EOF 当成功，不负责操作系统超时。
use super::task_lifecycle::{ArtifactEvidence, Stage, StopReason};
use serde::{Deserialize, Serialize};
use shield_core::diagnostic::FailureCode;
use std::io::{self, Read, Write};

pub(super) const MAX_FRAME: usize = 4096;
pub(super) const MAX_FRAMES: u32 = 128;
pub(super) const MAX_BYTES: usize = 256 * 1024;
const VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Peer {
    Parent,
    Worker,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum Message {
    Hello {},
    Planned {
        plan_digest: [u8; 32],
        certificate_sha256: [u8; 32],
        sources_sha256: [u8; 32],
    },
    Rejected {
        code: Rejection,
    },
    Start {
        plan_digest: [u8; 32],
    },
    Stop {
        reason: StopReason,
    },
    Progress {
        stage: Stage,
    },
    Ready {
        plan_digest: [u8; 32],
        artifact: ArtifactEvidence,
    },
    Failed {
        code: FailureCode,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub(super) enum Rejection {
    V4Unsupported,
    PlanBlocked,
    PlanMismatch,
    SourceChanged,
    InvalidRequest,
    CredentialUnavailable,
    ValidationFailed,
    ProtectionMissing,
    SignatureInvalid,
    CertificateMismatch,
    AlignmentInvalid,
    MaterialInvalid,
    ExecutorChanged,
    ToolchainChanged,
}

impl Message {
    fn sender(&self) -> Peer {
        match self {
            Self::Start { .. } | Self::Stop { .. } => Peer::Parent,
            _ => Peer::Worker,
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Envelope {
    protocol_version: u32,
    task_id: [u8; 16],
    sequence: u32,
    message: Message,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ProtocolError {
    EndOfStream,
    Truncated,
    InvalidFrame,
    UnsupportedVersion,
    TaskMismatch,
    Sequence,
    WrongDirection,
    BudgetExceeded,
    Transport,
    Closed,
}

struct Session {
    task_id: [u8; 16],
    sender: Peer,
    sequence: u32,
    bytes: usize,
    closed: bool,
}

impl Session {
    fn new(task_id: [u8; 16], sender: Peer) -> Self {
        Self {
            task_id,
            sender,
            sequence: 0,
            bytes: 0,
            closed: false,
        }
    }

    fn begin(&mut self) -> Result<(), ProtocolError> {
        if self.closed {
            return Err(ProtocolError::Closed);
        }
        // 任意失败均令会话不可复用，不尝试从原始日志或畸形帧中重新同步。
        self.closed = true;
        if self.sequence >= MAX_FRAMES {
            return Err(ProtocolError::BudgetExceeded);
        }
        Ok(())
    }

    fn budget(&self, len: usize) -> Result<(), ProtocolError> {
        if len == 0 || len > MAX_FRAME {
            return Err(ProtocolError::InvalidFrame);
        }
        if self.bytes + 4 + len > MAX_BYTES {
            return Err(ProtocolError::BudgetExceeded);
        }
        Ok(())
    }

    fn accept(&mut self, len: usize) {
        self.sequence += 1;
        self.bytes += 4 + len;
        self.closed = false;
    }
}

pub(super) struct FrameReader<R> {
    input: R,
    session: Session,
}

impl<R: Read> FrameReader<R> {
    pub fn new(input: R, task_id: [u8; 16], sender: Peer) -> Self {
        Self {
            input,
            session: Session::new(task_id, sender),
        }
    }

    pub fn read(&mut self) -> Result<Message, ProtocolError> {
        self.session.begin()?;
        let mut header = [0; 4];
        self.input
            .read_exact(&mut header)
            .map_err(|error| transport(error, ProtocolError::EndOfStream))?;
        let len = u32::from_be_bytes(header) as usize;
        // 在为外来数据分配内存前限制单帧及总字节；不能使用无界 read_line/read_to_end。
        self.session.budget(len)?;
        let mut bytes = vec![0; len];
        self.input
            .read_exact(&mut bytes)
            .map_err(|error| transport(error, ProtocolError::Truncated))?;
        let envelope: Envelope =
            serde_json::from_slice(&bytes).map_err(|_| ProtocolError::InvalidFrame)?;
        if envelope.protocol_version != VERSION {
            return Err(ProtocolError::UnsupportedVersion);
        }
        if envelope.task_id != self.session.task_id {
            return Err(ProtocolError::TaskMismatch);
        }
        if envelope.sequence != self.session.sequence {
            return Err(ProtocolError::Sequence);
        }
        if envelope.message.sender() != self.session.sender {
            return Err(ProtocolError::WrongDirection);
        }
        self.session.accept(len);
        Ok(envelope.message)
    }
}

pub(super) struct FrameWriter<W> {
    output: W,
    session: Session,
}

impl<W: Write> FrameWriter<W> {
    pub fn new(output: W, task_id: [u8; 16], sender: Peer) -> Self {
        Self {
            output,
            session: Session::new(task_id, sender),
        }
    }

    pub fn send(&mut self, message: Message) -> Result<(), ProtocolError> {
        self.session.begin()?;
        if message.sender() != self.session.sender {
            return Err(ProtocolError::WrongDirection);
        }
        // 消息只含固定长度摘要/标识、有限枚举、整数及布尔值，不接受任意字符串或日志。
        let bytes = serde_json::to_vec(&Envelope {
            protocol_version: VERSION,
            task_id: self.session.task_id,
            sequence: self.session.sequence,
            message,
        })
        .map_err(|_| ProtocolError::InvalidFrame)?;
        self.session.budget(bytes.len())?;
        self.output
            .write_all(&(bytes.len() as u32).to_be_bytes())
            .map_err(|_| ProtocolError::Transport)?;
        self.output
            .write_all(&bytes)
            .map_err(|_| ProtocolError::Transport)?;
        self.output.flush().map_err(|_| ProtocolError::Transport)?;
        self.session.accept(bytes.len());
        Ok(())
    }
}

fn transport(error: io::Error, eof: ProtocolError) -> ProtocolError {
    if error.kind() == io::ErrorKind::UnexpectedEof {
        eof
    } else {
        ProtocolError::Transport
    }
}

#[cfg(test)]
#[path = "worker_protocol_tests.rs"]
mod tests;

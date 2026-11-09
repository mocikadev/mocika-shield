use super::*;
use serde_json::{json, Value};
use std::io::Cursor;

const TASK: [u8; 16] = [8; 16];

fn envelope() -> Value {
    json!({ "protocol_version": 1, "task_id": TASK, "sequence": 0, "message": { "type": "hello" } })
}

fn frame(value: Value) -> Vec<u8> {
    let bytes = serde_json::to_vec(&value).unwrap();
    raw_frame(&bytes)
}

fn raw_frame(bytes: &[u8]) -> Vec<u8> {
    let mut frame = (bytes.len() as u32).to_be_bytes().to_vec();
    frame.extend_from_slice(bytes);
    frame
}

#[test]
fn 双向有限消息逐帧往返且边界清晰() {
    let evidence = ArtifactEvidence {
        bytes: 99,
        sha256: [2; 32],
        certificate_sha256: [3; 32],
        signature_checked: true,
        protection_checked: true,
        alignment_checked: true,
        content_sha256: None,
    };
    for (peer, messages) in [
        (
            Peer::Parent,
            vec![
                Message::Start {
                    plan_digest: [1; 32],
                },
                Message::Stop {
                    reason: StopReason::Finalize,
                },
            ],
        ),
        (
            Peer::Worker,
            vec![
                Message::Hello {},
                Message::Planned {
                    plan_digest: [1; 32],
                    certificate_sha256: [3; 32],
                    sources_sha256: [4; 32],
                },
                Message::Rejected {
                    code: Rejection::V4Unsupported,
                },
                Message::Rejected {
                    code: Rejection::ExecutorChanged,
                },
                Message::Rejected {
                    code: Rejection::ToolchainChanged,
                },
                Message::Progress {
                    stage: Stage::Prepare,
                },
                Message::Ready {
                    plan_digest: [1; 32],
                    artifact: evidence,
                },
                Message::Failed {
                    code: FailureCode::SigningFailed,
                },
            ],
        ),
    ] {
        let mut bytes = Vec::new();
        let mut writer = FrameWriter::new(&mut bytes, TASK, peer);
        for message in &messages {
            writer.send(message.clone()).unwrap();
        }
        let mut reader = FrameReader::new(Cursor::new(bytes), TASK, peer);
        for message in messages {
            assert_eq!(reader.read().unwrap(), message);
        }
        assert_eq!(reader.read(), Err(ProtocolError::EndOfStream));
        assert_eq!(reader.read(), Err(ProtocolError::Closed));
    }
}

#[test]
fn 单帧超限和总预算超限在读内容前拒绝() {
    let mut cursor = Cursor::new((MAX_FRAME as u32 + 1).to_be_bytes().to_vec());
    assert_eq!(
        FrameReader::new(&mut cursor, TASK, Peer::Worker).read(),
        Err(ProtocolError::InvalidFrame)
    );
    assert_eq!(cursor.position(), 4);
    let mut bytes = serde_json::to_vec(&envelope()).unwrap();
    bytes.resize(MAX_FRAME, b' ');
    let mut cursor = Cursor::new(raw_frame(&bytes));
    let mut reader = FrameReader::new(&mut cursor, TASK, Peer::Worker);
    reader.session.bytes = MAX_BYTES - MAX_FRAME;
    assert_eq!(reader.read(), Err(ProtocolError::BudgetExceeded));
    assert_eq!(cursor.position(), 4);
}

#[test]
fn 不完整首部内容空帧和原始日志全部失败且不能重新同步() {
    for (bytes, expected) in [
        (vec![], ProtocolError::EndOfStream),
        (vec![0, 0], ProtocolError::EndOfStream),
        (raw_frame(b""), ProtocolError::InvalidFrame),
        (vec![0, 0, 0, 2, b'{'], ProtocolError::Truncated),
        (
            b"Java error should not become a message".to_vec(),
            ProtocolError::InvalidFrame,
        ),
        (raw_frame(&[0xff]), ProtocolError::InvalidFrame),
    ] {
        let mut reader = FrameReader::new(Cursor::new(bytes), TASK, Peer::Worker);
        assert_eq!(reader.read(), Err(expected));
        assert_eq!(reader.read(), Err(ProtocolError::Closed));
    }
}

#[test]
fn 版本任务标识序号与消息方向逐一校验() {
    for (key, value, error) in [
        (
            "protocol_version",
            json!(2),
            ProtocolError::UnsupportedVersion,
        ),
        ("task_id", json!(vec![0; 16]), ProtocolError::TaskMismatch),
        ("sequence", json!(1), ProtocolError::Sequence),
        (
            "message",
            json!({"type":"stop","reason":"cancel"}),
            ProtocolError::WrongDirection,
        ),
    ] {
        let mut value_envelope = envelope();
        value_envelope[key] = value;
        let mut reader = FrameReader::new(Cursor::new(frame(value_envelope)), TASK, Peer::Worker);
        assert_eq!(reader.read(), Err(error));
        assert_eq!(reader.read(), Err(ProtocolError::Closed));
    }
    let mut writer = FrameWriter::new(Vec::new(), TASK, Peer::Worker);
    assert_eq!(
        writer.send(Message::Stop {
            reason: StopReason::Cancel
        }),
        Err(ProtocolError::WrongDirection)
    );
    assert!(writer.output.is_empty());
    assert_eq!(writer.send(Message::Hello {}), Err(ProtocolError::Closed));
}

#[test]
fn 未知字段日志密码路径别名及未知枚举不能进入消息() {
    let mut cases = Vec::new();
    for field in ["password", "path", "alias", "log"] {
        let mut extra = envelope();
        extra[field] = json!("禁止透传的输入");
        cases.push(extra);
        let mut extra = envelope();
        extra["message"][field] = json!("禁止透传的输入");
        cases.push(extra);
    }
    for message in [
        json!({"type":"shell","command":"任意命令"}),
        json!({"type":"progress","stage":"任意阶段"}),
        json!({"type":"failed","code":"任意错误"}),
    ] {
        let mut value = envelope();
        value["message"] = message;
        cases.push(value);
    }
    for value in cases {
        let mut reader = FrameReader::new(Cursor::new(frame(value)), TASK, Peer::Worker);
        assert_eq!(reader.read(), Err(ProtocolError::InvalidFrame));
    }
}

#[test]
fn 重复字段尾随数据错误标识长度及过深嵌套拒绝() {
    let canonical = serde_json::to_string(&envelope()).unwrap();
    let duplicate = canonical.replacen("\"sequence\":0", "\"sequence\":0,\"sequence\":0", 1);
    let duplicate_type = canonical.replacen(
        "\"type\":\"hello\"",
        "\"type\":\"hello\",\"type\":\"hello\"",
        1,
    );
    let mut short = envelope();
    short["task_id"] = json!([0]);
    let nested = format!("{}0{}", "[".repeat(150), "]".repeat(150));
    for bytes in [
        duplicate.into_bytes(),
        duplicate_type.into_bytes(),
        format!("{canonical}{{}}").into_bytes(),
        serde_json::to_vec(&short).unwrap(),
        nested.into_bytes(),
    ] {
        assert_eq!(
            FrameReader::new(Cursor::new(raw_frame(&bytes)), TASK, Peer::Worker).read(),
            Err(ProtocolError::InvalidFrame)
        );
    }
}

#[test]
fn 候选证据未知字段和错误布尔类型不能绕过校验() {
    let mut bytes = Vec::new();
    FrameWriter::new(&mut bytes, TASK, Peer::Worker)
        .send(Message::Ready {
            plan_digest: [1; 32],
            artifact: ArtifactEvidence {
                bytes: 1,
                sha256: [2; 32],
                certificate_sha256: [3; 32],
                signature_checked: true,
                protection_checked: true,
                alignment_checked: true,
                content_sha256: None,
            },
        })
        .unwrap();
    let canonical: Value = serde_json::from_slice(&bytes[4..]).unwrap();
    for invalid in 0..3 {
        let mut value = canonical.clone();
        match invalid {
            0 => value["message"]["artifact"]["log"] = json!("不得透传"),
            1 => value["message"]["artifact"]["signature_checked"] = json!("true"),
            _ => value["message"]["artifact"]["sha256"] = json!([0]),
        }
        assert_eq!(
            FrameReader::new(Cursor::new(frame(value)), TASK, Peer::Worker).read(),
            Err(ProtocolError::InvalidFrame)
        );
    }
}

#[test]
fn 连续合法大帧仍受总字节预算约束() {
    let accepted = MAX_BYTES / (MAX_FRAME + 4);
    let mut stream = Vec::new();
    for sequence in 0..=accepted {
        let mut value = envelope();
        value["sequence"] = json!(sequence);
        let mut bytes = serde_json::to_vec(&value).unwrap();
        bytes.resize(MAX_FRAME, b' ');
        stream.extend_from_slice(&raw_frame(&bytes));
    }
    let mut reader = FrameReader::new(Cursor::new(stream), TASK, Peer::Worker);
    for _ in 0..accepted {
        reader.read().unwrap();
    }
    assert_eq!(reader.read(), Err(ProtocolError::BudgetExceeded));
    assert_eq!(reader.read(), Err(ProtocolError::Closed));
}

#[test]
fn 写入回执失败不代表消息未送达且绝不自动重发() {
    struct FlushFailed {
        bytes: Vec<u8>,
    }
    impl Write for FlushFailed {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.bytes.extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Err(io::ErrorKind::BrokenPipe.into())
        }
    }
    let mut writer = FrameWriter::new(FlushFailed { bytes: Vec::new() }, TASK, Peer::Parent);
    assert_eq!(
        writer.send(Message::Start {
            plan_digest: [1; 32]
        }),
        Err(ProtocolError::Transport)
    );
    let received = writer.output.bytes.clone();
    assert!(matches!(
        FrameReader::new(Cursor::new(received), TASK, Peer::Parent).read(),
        Ok(Message::Start { .. })
    ));
    assert_eq!(
        writer.send(Message::Start {
            plan_digest: [1; 32]
        }),
        Err(ProtocolError::Closed)
    );
}

#[test]
fn 重放消息和消息数量上限均拒绝() {
    let mut replay = frame(envelope());
    replay.extend_from_slice(&frame(envelope()));
    let mut reader = FrameReader::new(Cursor::new(replay), TASK, Peer::Worker);
    assert_eq!(reader.read(), Ok(Message::Hello {}));
    assert_eq!(reader.read(), Err(ProtocolError::Sequence));
    let mut bytes = Vec::new();
    let mut writer = FrameWriter::new(&mut bytes, TASK, Peer::Worker);
    for _ in 0..MAX_FRAMES {
        writer.send(Message::Hello {}).unwrap();
    }
    assert_eq!(
        writer.send(Message::Hello {}),
        Err(ProtocolError::BudgetExceeded)
    );
    let mut reader = FrameReader::new(Cursor::new(bytes), TASK, Peer::Worker);
    for _ in 0..MAX_FRAMES {
        reader.read().unwrap();
    }
    assert_eq!(reader.read(), Err(ProtocolError::BudgetExceeded));
}

#[test]
fn 分片读取与写入失败仍保持有界协议() {
    struct Fragmented(Cursor<Vec<u8>>);
    impl Read for Fragmented {
        fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
            let len = buffer.len().min(1);
            self.0.read(&mut buffer[..len])
        }
    }
    assert_eq!(
        FrameReader::new(
            Fragmented(Cursor::new(frame(envelope()))),
            TASK,
            Peer::Worker
        )
        .read(),
        Ok(Message::Hello {})
    );
    struct Broken;
    impl Write for Broken {
        fn write(&mut self, _: &[u8]) -> io::Result<usize> {
            Err(io::ErrorKind::BrokenPipe.into())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    let mut writer = FrameWriter::new(Broken, TASK, Peer::Worker);
    assert_eq!(
        writer.send(Message::Hello {}),
        Err(ProtocolError::Transport)
    );
    assert_eq!(writer.send(Message::Hello {}), Err(ProtocolError::Closed));
}

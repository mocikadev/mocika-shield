//! 候选组件联动：整组停止由测试显式模拟，不是原生进程、真实 APK 或签名回归。
use super::publication::{Publication, PublicationCheckpoint, PublicationError};
use super::task_lifecycle::{ArtifactEvidence, Failure, Output, Phase, Status, Task};
use super::worker_protocol::{FrameReader, FrameWriter, Message, Peer};
use sha2::{Digest, Sha256};
use std::fs;
use std::io::Cursor;
use std::time::{Duration, Instant};

const PLAN: [u8; 32] = [1; 32];
const CERT: [u8; 32] = [2; 32];
const TASK_ID: [u8; 16] = [3; 16];
const BYTES: &[u8] = "只作组件契约验证，不是 APK".as_bytes();

fn prepared(now: Instant) -> (tempfile::TempDir, Publication, Task) {
    let root = tempfile::tempdir().unwrap();
    let publication = Publication::new(&root.path().join("output.apk")).unwrap();
    fs::write(publication.work_dir().unwrap().join("signed.apk"), BYTES).unwrap();
    let evidence = ArtifactEvidence {
        bytes: BYTES.len() as u64,
        sha256: Sha256::digest(BYTES).into(),
        certificate_sha256: CERT,
        signature_checked: true,
        protection_checked: true,
        alignment_checked: true,
    };
    let mut messages = Vec::new();
    let mut writer = FrameWriter::new(&mut messages, TASK_ID, Peer::Worker);
    writer.send(Message::Hello {}).unwrap();
    writer
        .send(Message::Ready {
            plan_digest: PLAN,
            artifact: evidence,
        })
        .unwrap();
    let mut reader = FrameReader::new(Cursor::new(messages), TASK_ID, Peer::Worker);
    let mut task = Task::new(now, Duration::from_secs(120), PLAN, CERT).unwrap();
    task.isolation_confirmed(now).unwrap();
    assert_eq!(reader.read().unwrap(), Message::Hello {});
    task.hello(now).unwrap();
    task.start(now).unwrap();
    let Message::Ready {
        plan_digest,
        artifact,
    } = reader.read().unwrap()
    else {
        panic!("应收到有限候选消息")
    };
    task.ready(now, plan_digest, artifact).unwrap();
    assert_eq!(task.phase(), Phase::Stopping);
    assert!(!task.can_cleanup());
    assert!(task.artifact().is_none());
    assert!(task.take_outcome().is_none());
    (root, publication, task)
}

#[test]
fn 协议候选必须经过停止验收及提交点才返回成功() {
    let now = Instant::now();
    let (root, mut publication, mut task) = prepared(now);
    task.tree_stopped(now).unwrap();
    task.verified(now, true).unwrap();
    let expected = task.artifact().unwrap().sha256;
    let receipt = publication
        .publish_after_stop(expected, |point| {
            match point {
                PublicationCheckpoint::Copy => task.checkpoint(now),
                PublicationCheckpoint::BeforeCommit => task.begin_publish(now),
            }
            .map_err(|_| PublicationError::Cancelled)
        })
        .unwrap();
    assert_eq!(receipt.bytes, BYTES.len() as u64);
    assert_eq!(receipt.sha256, expected);
    task.committed(
        receipt.location_confirmed,
        receipt.directory_synced,
        receipt.cleanup_complete,
    )
    .unwrap();
    assert_eq!(task.take_outcome().unwrap().status, Status::Succeeded);
    assert!(task.take_outcome().is_none());
    assert_eq!(fs::read(root.path().join("output.apk")).unwrap(), BYTES);
    assert!(task.can_cleanup());
}

#[test]
fn 复制期间取消或期限到达不进入提交临界区() {
    let now = Instant::now();
    for expired in [false, true] {
        let (root, mut publication, mut task) = prepared(now);
        task.tree_stopped(now).unwrap();
        task.verified(now, true).unwrap();
        let expected = task.artifact().unwrap().sha256;
        let result = publication.publish_after_stop(expected, |point| {
            assert_eq!(point, PublicationCheckpoint::Copy);
            if expired {
                task.checkpoint(now + Duration::from_secs(120))
                    .map_err(|_| PublicationError::TimedOut)
            } else {
                task.cancel(now).unwrap();
                task.checkpoint(now)
                    .map_err(|_| PublicationError::Cancelled)
            }
        });
        assert!(result.is_err());
        let outcome = task.take_outcome().unwrap();
        assert_eq!(
            outcome.status,
            if expired {
                Status::TimedOut
            } else {
                Status::Cancelled
            }
        );
        assert_eq!(outcome.output, Output::NotPublished);
        assert!(task.can_cleanup());
        assert!(publication.cleanup());
        assert_eq!(fs::read_dir(root.path()).unwrap().count(), 0);
    }
}

#[test]
fn 临界区内取消不能覆盖已经提交的事实() {
    let now = Instant::now();
    let (root, mut publication, mut task) = prepared(now);
    task.tree_stopped(now).unwrap();
    task.verified(now, true).unwrap();
    let expected = task.artifact().unwrap().sha256;
    let receipt = publication
        .publish_after_stop(expected, |point| {
            match point {
                PublicationCheckpoint::Copy => task.checkpoint(now).unwrap(),
                PublicationCheckpoint::BeforeCommit => {
                    task.begin_publish(now).unwrap();
                    task.cancel(now).unwrap();
                    assert!(task.take_outcome().is_none());
                }
            }
            Ok(())
        })
        .unwrap();
    task.committed(
        receipt.location_confirmed,
        receipt.directory_synced,
        receipt.cleanup_complete,
    )
    .unwrap();
    assert_eq!(task.take_outcome().unwrap().status, Status::Succeeded);
    assert_eq!(fs::read(root.path().join("output.apk")).unwrap(), BYTES);
}

#[test]
fn 最后检查后被抢占也由原子原语拒绝覆盖() {
    let now = Instant::now();
    let (root, mut publication, mut task) = prepared(now);
    task.tree_stopped(now).unwrap();
    task.verified(now, true).unwrap();
    let expected = task.artifact().unwrap().sha256;
    let target = root.path().join("output.apk");
    let result = publication.publish_after_stop(expected, |point| {
        if point == PublicationCheckpoint::BeforeCommit {
            task.begin_publish(now).unwrap();
            fs::write(&target, "另一个任务的结果").unwrap();
        } else {
            task.checkpoint(now).unwrap();
        }
        Ok(())
    });
    assert!(matches!(result, Err(PublicationError::OutputExists)));
    task.publication_failed(now).unwrap();
    assert_eq!(
        task.take_outcome().unwrap().status,
        Status::Failed(Failure::Publication)
    );
    assert!(task.can_cleanup());
    assert!(publication.cleanup());
    assert_eq!(fs::read(&target).unwrap(), "另一个任务的结果".as_bytes());
}

#[test]
fn 畸形消息之后候选失效且停止确认前不清理() {
    let now = Instant::now();
    let (root, mut publication, mut task) = prepared(now);
    let mut invalid = FrameReader::new(
        Cursor::new("原始错误日志不是协议".as_bytes().to_vec()),
        TASK_ID,
        Peer::Worker,
    );
    assert!(invalid.read().is_err());
    task.protocol_failed(now).unwrap();
    assert!(!task.can_cleanup());
    assert!(task.take_outcome().is_none());
    task.tree_stopped(now).unwrap();
    assert!(task.can_cleanup());
    assert_eq!(
        task.take_outcome().unwrap().status,
        Status::Failed(Failure::Protocol)
    );
    assert!(publication.cleanup());
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 0);
}

#[test]
fn 固化摘要失败在进入临界区前返回未提交() {
    let now = Instant::now();
    let (root, mut publication, mut task) = prepared(now);
    task.tree_stopped(now).unwrap();
    task.verified(now, true).unwrap();
    let result = publication.publish_after_stop([0; 32], |point| {
        assert_eq!(point, PublicationCheckpoint::Copy);
        task.checkpoint(now).unwrap();
        Ok(())
    });
    assert!(matches!(result, Err(PublicationError::DigestMismatch)));
    assert_eq!(task.phase(), Phase::ReadyToPublish);
    task.publication_failed(now).unwrap();
    assert_eq!(
        task.take_outcome().unwrap().status,
        Status::Failed(Failure::Publication)
    );
    assert!(task.can_cleanup());
    assert!(publication.cleanup());
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 0);
}

#[test]
#[ignore = "需要普通用户权限和目录权限文件系统，显式验证提交调用错误保守报未知"]
fn 提交调用异常保留未知及本次材料而非重试() {
    use std::os::unix::fs::PermissionsExt;
    let now = Instant::now();
    let (root, mut publication, mut task) = prepared(now);
    let work = publication.work_dir().unwrap().to_owned();
    task.tree_stopped(now).unwrap();
    task.verified(now, true).unwrap();
    let expected = task.artifact().unwrap().sha256;
    let result = publication.publish_after_stop(expected, |point| {
        if point == PublicationCheckpoint::BeforeCommit {
            task.begin_publish(now).unwrap();
            fs::set_permissions(root.path(), fs::Permissions::from_mode(0o500)).unwrap();
        } else {
            task.checkpoint(now).unwrap();
        }
        Ok(())
    });
    fs::set_permissions(root.path(), fs::Permissions::from_mode(0o700)).unwrap();
    assert!(
        matches!(result, Err(PublicationError::CommitUnconfirmed)),
        "请用普通用户及支持目录权限的文件系统运行"
    );
    assert!(matches!(
        publication.publish_after_stop(expected, |_| panic!("未知提交不得重试")),
        Err(PublicationError::CommitUnconfirmed)
    ));
    task.commit_unconfirmed().unwrap();
    assert_eq!(task.take_outcome().unwrap().output, Output::Uncertain);
    assert!(!task.can_cleanup());
    assert!(!publication.cleanup());
    drop(publication);
    assert!(work.join("signed.apk").is_file());
    assert_eq!(fs::read_dir(&work).unwrap().count(), 2);
    // 不确定提交的临时材料由独立测试根目录最终回收，产品代码不进行额外清理。
}

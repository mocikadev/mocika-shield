//! 提交前复核的真实检查点测试；不启动外部进程、不改最终产物。
use super::*;
use crate::automation::task_lifecycle::{ArtifactEvidence, Status};

fn ready(timeout: Duration) -> (Task, Instant) {
    let now = Instant::now();
    let mut task = Task::new(now, timeout, [1; 32], [2; 32]).unwrap();
    task.isolation_confirmed(Instant::now()).unwrap();
    task.hello(Instant::now()).unwrap();
    task.start(Instant::now()).unwrap();
    task.ready(
        Instant::now(),
        [1; 32],
        ArtifactEvidence {
            bytes: 1,
            sha256: [3; 32],
            certificate_sha256: [2; 32],
            signature_checked: true,
            protection_checked: true,
            alignment_checked: true,
            content_sha256: None,
        },
    )
    .unwrap();
    task.tree_stopped(Instant::now()).unwrap();
    task.verified(Instant::now(), true).unwrap();
    (task, now + timeout)
}

#[test]
fn 最后复核期间取消必须保留取消终态而非发布失败() {
    let (mut task, deadline) = ready(Duration::from_secs(5));
    let cancel = AtomicBool::new(false);
    let outcome = publication_checkpoint(
        &mut task,
        PublicationCheckpoint::BeforeCommit,
        deadline,
        &cancel,
        || {
            cancel.store(true, Ordering::SeqCst);
            Err(error("TASK_CANCELLED"))
        },
    );
    assert!(matches!(outcome, Err(PublicationError::Cancelled)));
    let _ = task.publication_failed(Instant::now());
    assert_eq!(task.take_outcome().unwrap().status, Status::Cancelled);
}

#[test]
fn 最后复核期间真实到期保留超时终态() {
    let (mut task, deadline) = ready(Duration::from_millis(200));
    let cancel = AtomicBool::new(false);
    let outcome = publication_checkpoint(
        &mut task,
        PublicationCheckpoint::BeforeCommit,
        deadline,
        &cancel,
        || {
            std::thread::sleep(
                deadline.saturating_duration_since(Instant::now()) + Duration::from_millis(1),
            );
            Err(error("TASK_TIMEOUT"))
        },
    );
    assert!(matches!(outcome, Err(PublicationError::TimedOut)));
    let _ = task.publication_failed(Instant::now());
    assert_eq!(task.take_outcome().unwrap().status, Status::TimedOut);
}

#[test]
fn 已请求取消不再读取提交材料() {
    let (mut task, deadline) = ready(Duration::from_secs(5));
    let outcome = publication_checkpoint(
        &mut task,
        PublicationCheckpoint::BeforeCommit,
        deadline,
        &AtomicBool::new(true),
        || panic!("已取消后不应继续复核材料"),
    );
    assert!(matches!(outcome, Err(PublicationError::Cancelled)));
    assert_eq!(task.take_outcome().unwrap().status, Status::Cancelled);
}

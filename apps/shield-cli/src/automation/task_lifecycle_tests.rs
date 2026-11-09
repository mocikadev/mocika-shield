use super::*;

const PLAN: [u8; 32] = [7; 32];
const CERT: [u8; 32] = [9; 32];

fn evidence() -> ArtifactEvidence {
    ArtifactEvidence {
        bytes: 123,
        sha256: [3; 32],
        certificate_sha256: CERT,
        signature_checked: true,
        protection_checked: true,
        alignment_checked: true,
        content_sha256: None,
    }
}

#[test]
fn 两种任务的证据不得互换且签名必须保留指定内容() {
    let now = Instant::now();
    for invalid in 0..5 {
        let mut signing =
            Task::new_sign(now, Duration::from_secs(120), PLAN, CERT, [4; 32]).unwrap();
        signing.hello(now).unwrap();
        signing.isolation_confirmed(now).unwrap();
        signing.start(now).unwrap();
        let mut artifact = evidence();
        artifact.protection_checked = false;
        artifact.content_sha256 = Some([4; 32]);
        match invalid {
            0 => artifact.content_sha256 = None,
            1 => artifact.content_sha256 = Some([5; 32]),
            2 => artifact.protection_checked = true,
            3 => artifact.signature_checked = false,
            _ => artifact.certificate_sha256 = [0; 32],
        }
        assert!(signing.ready(now, PLAN, artifact).is_err());
    }
    let mut signing = Task::new_sign(now, Duration::from_secs(120), PLAN, CERT, [4; 32]).unwrap();
    signing.hello(now).unwrap();
    signing.isolation_confirmed(now).unwrap();
    signing.start(now).unwrap();
    let mut artifact = evidence();
    artifact.protection_checked = false;
    artifact.content_sha256 = Some([4; 32]);
    signing.ready(now, PLAN, artifact).unwrap();
    assert!(!signing.can_cleanup());
    signing.tree_stopped(now).unwrap();
    assert_eq!(signing.phase(), Phase::Finalizing);
    signing.verified(now, true).unwrap();
    signing.begin_publish(now).unwrap();
    signing.cancel(now).unwrap();
    signing.committed(true, true, true).unwrap();
    signing.cancel(now).unwrap();
    assert_eq!(signing.take_outcome().unwrap().status, Status::Succeeded);
    assert!(running(now).ready(now, PLAN, artifact).is_err());
    artifact.protection_checked = true;
    assert!(running(now).ready(now, PLAN, artifact).is_err());
}

#[test]
fn 签名任务停止未确认时仍保留未知且不得清理() {
    let now = Instant::now();
    let mut signing = Task::new_sign(now, Duration::from_secs(120), PLAN, CERT, [4; 32]).unwrap();
    signing.cancel(now).unwrap();
    signing.stop_unconfirmed(now).unwrap();
    assert_eq!(signing.take_outcome().unwrap().status, Status::Unknown);
    assert!(!signing.can_cleanup());
}

#[test]
fn 两类任务启动前父端复核失败不伪造工作进程错误且必须等待停止() {
    let now = Instant::now();
    for sign in [false, true] {
        for hello in [false, true] {
            let mut task = if sign {
                Task::new_sign(now, Duration::from_secs(120), PLAN, CERT, [4; 32]).unwrap()
            } else {
                task(now)
            };
            task.isolation_confirmed(now).unwrap();
            if hello {
                task.hello(now).unwrap();
            }
            task.preparation_failed(now).unwrap();
            assert_eq!(task.phase(), Phase::Stopping);
            assert!(task.take_outcome().is_none());
            assert!(!task.can_cleanup());
            task.tree_stopped(now).unwrap();
            assert_eq!(
                task.take_outcome().unwrap().status,
                Status::Failed(Failure::Verification)
            );
            assert!(task.can_cleanup());
        }
    }
    assert!(running(now).preparation_failed(now).is_err());
}

fn task(now: Instant) -> Task {
    Task::new(now, Duration::from_secs(120), PLAN, CERT).unwrap()
}

fn running(now: Instant) -> Task {
    let mut task = task(now);
    task.hello(now).unwrap();
    task.isolation_confirmed(now).unwrap();
    task.start(now).unwrap();
    task
}

fn finalizing(now: Instant) -> Task {
    let mut task = running(now);
    task.ready(now, PLAN, evidence()).unwrap();
    assert_eq!(task.stop_reason(), Some(StopReason::Finalize));
    assert!(task.take_outcome().is_none());
    task.tree_stopped(now).unwrap();
    task
}

fn publishing(now: Instant) -> Task {
    let mut task = finalizing(now);
    task.verified(now, true).unwrap();
    task.begin_publish(now).unwrap();
    task
}

#[test]
fn 拒绝空期限溢出和时间倒退且不延长期限() {
    let now = Instant::now();
    assert!(matches!(
        Task::new(now, Duration::ZERO, PLAN, CERT),
        Err(TransitionError::InvalidTime)
    ));
    assert!(matches!(
        Task::new(now, Duration::MAX, PLAN, CERT),
        Err(TransitionError::InvalidTime)
    ));
    let mut task = running(now);
    assert_eq!(
        task.checkpoint(now - Duration::from_secs(1)),
        Err(TransitionError::InvalidTime)
    );
    assert_eq!(
        task.checkpoint(now + Duration::from_secs(120)),
        Err(TransitionError::Interrupted)
    );
    assert_eq!(task.stop_reason(), Some(StopReason::Timeout));
}

#[test]
fn 握手和平台隔离两者缺一都不能授权开始() {
    let now = Instant::now();
    let mut task = task(now);
    assert_eq!(task.phase(), Phase::Starting);
    assert!(task.start(now).is_err());
    task.hello(now).unwrap();
    assert_eq!(task.phase(), Phase::Handshaken);
    assert!(task.start(now).is_err());
    task.isolation_confirmed(now).unwrap();
    task.start(now).unwrap();
    assert_eq!(task.phase(), Phase::Running);
    assert!(task.isolation_confirmed(now).is_err());
}

#[test]
fn 重复握手与提前进度拒绝且需要整组停止才有失败终态() {
    let now = Instant::now();
    for duplicate in [false, true] {
        let mut task = task(now);
        if duplicate {
            task.hello(now).unwrap();
            assert!(task.hello(now).is_err());
        } else {
            assert!(task.progress(now, Stage::Prepare).is_err());
        }
        assert_eq!(task.phase(), Phase::Stopping);
        assert!(!task.can_cleanup());
        assert!(task.take_outcome().is_none());
        task.tree_stopped(now).unwrap();
        assert_eq!(
            task.take_outcome().unwrap().status,
            Status::Failed(Failure::Protocol)
        );
        assert!(task.can_cleanup());
    }
}

#[test]
fn 有限进度只能前进且不会刷新总期限() {
    let now = Instant::now();
    let mut task = running(now);
    task.progress(now + Duration::from_secs(1), Stage::Prepare)
        .unwrap();
    task.progress(now + Duration::from_secs(59), Stage::Repack)
        .unwrap();
    task.progress(now + Duration::from_secs(119), Stage::Verify)
        .unwrap();
    assert!(task.checkpoint(now + Duration::from_secs(120)).is_err());
    assert_eq!(task.stop_reason(), Some(StopReason::Timeout));
    assert!(task.take_outcome().is_none());
    task.tree_stopped(now + Duration::from_secs(121)).unwrap();
    assert_eq!(task.take_outcome().unwrap().status, Status::TimedOut);
}

#[test]
fn 重复和倒退阶段均失败关闭() {
    let now = Instant::now();
    for stage in [Stage::Sign, Stage::Unpack] {
        let mut task = running(now);
        task.progress(now, Stage::Sign).unwrap();
        assert!(task.progress(now, stage).is_err());
        task.tree_stopped(now).unwrap();
        assert_eq!(
            task.take_outcome().unwrap().status,
            Status::Failed(Failure::Protocol)
        );
    }
}

#[test]
fn 候选必须绑定计划证书和全部验收声明() {
    let now = Instant::now();
    for invalid in 0..6 {
        let mut task = running(now);
        let mut artifact = evidence();
        let mut plan = PLAN;
        match invalid {
            0 => plan = [0; 32],
            1 => artifact.certificate_sha256 = [0; 32],
            2 => artifact.bytes = 0,
            3 => artifact.signature_checked = false,
            4 => artifact.protection_checked = false,
            _ => artifact.alignment_checked = false,
        }
        assert!(task.ready(now, plan, artifact).is_err());
        assert!(task.artifact().is_none());
        assert!(task.take_outcome().is_none());
        task.tree_stopped(now).unwrap();
        assert_eq!(
            task.take_outcome().unwrap().status,
            Status::Failed(Failure::InvalidEvidence)
        );
    }
}

#[test]
fn 工作进程就绪和正常断流不能代替整组停止及父进程验收() {
    let now = Instant::now();
    let mut task = running(now);
    task.ready(now, PLAN, evidence()).unwrap();
    task.channel_closed(now).unwrap();
    assert!(task.verified(now, true).is_err());
    assert!(task.begin_publish(now).is_err());
    assert!(!task.can_cleanup());
    assert!(task.take_outcome().is_none());
    task.tree_stopped(now).unwrap();
    assert_eq!(task.phase(), Phase::Finalizing);
    assert_eq!(task.artifact(), Some(evidence()));
    assert!(task.begin_publish(now).is_err());
    task.verified(now, true).unwrap();
    assert_eq!(task.phase(), Phase::ReadyToPublish);
}

#[test]
fn 父进程验收失败不能交付() {
    let now = Instant::now();
    let mut task = finalizing(now);
    task.verified(now, false).unwrap();
    assert!(task.begin_publish(now).is_err());
    assert_eq!(
        task.take_outcome().unwrap().status,
        Status::Failed(Failure::Verification)
    );
    assert!(task.can_cleanup());
}

#[test]
fn 未确认停止只能报未知并保留活动暂存() {
    let now = Instant::now();
    let mut task = running(now);
    task.cancel(now).unwrap();
    assert_eq!(task.stop_reason(), Some(StopReason::Cancel));
    assert!(task.take_outcome().is_none());
    task.stop_unconfirmed(now).unwrap();
    assert_eq!(
        task.take_outcome(),
        Some(Outcome {
            status: Status::Unknown,
            output: Output::NotPublished
        })
    );
    assert!(!task.can_cleanup());
    assert!(task.start(now).is_err());
}

#[test]
fn 意外退出与协议失败均须停止确认() {
    let now = Instant::now();
    for protocol in [false, true] {
        let mut task = running(now);
        if protocol {
            task.protocol_failed(now).unwrap();
        } else {
            task.channel_closed(now).unwrap();
        }
        assert!(task.take_outcome().is_none());
        task.tree_stopped(now).unwrap();
        assert_eq!(
            task.take_outcome().unwrap().status,
            Status::Failed(if protocol {
                Failure::Protocol
            } else {
                Failure::UnexpectedExit
            })
        );
    }
    let mut task = running(now);
    task.tree_stopped(now).unwrap();
    assert_eq!(
        task.take_outcome().unwrap().status,
        Status::Failed(Failure::UnexpectedExit)
    );
}

#[test]
fn 故障不会被后来取消遮盖() {
    let now = Instant::now();
    let mut task = running(now);
    task.worker_failed(now, FailureCode::SigningFailed).unwrap();
    task.cancel(now).unwrap();
    task.protocol_failed(now).unwrap();
    task.tree_stopped(now).unwrap();
    assert_eq!(
        task.take_outcome().unwrap().status,
        Status::Failed(Failure::Worker(FailureCode::SigningFailed))
    );
}

#[test]
fn 取消和超时保留最先观察到的原因() {
    let now = Instant::now();
    let expired = now + Duration::from_secs(120);
    let mut cancelled = running(now);
    cancelled.cancel(now).unwrap();
    cancelled.tree_stopped(expired).unwrap();
    assert_eq!(cancelled.take_outcome().unwrap().status, Status::Cancelled);
    let mut timed_out = running(now);
    timed_out.cancel(expired).unwrap();
    timed_out.tree_stopped(expired).unwrap();
    assert_eq!(timed_out.take_outcome().unwrap().status, Status::TimedOut);
}

#[test]
fn 准备阶段也受总期限约束() {
    let now = Instant::now();
    let mut task = task(now);
    assert!(task.hello(now + Duration::from_secs(120)).is_err());
    assert!(task.start(now + Duration::from_secs(120)).is_err());
    task.tree_stopped(now + Duration::from_secs(120)).unwrap();
    assert_eq!(task.take_outcome().unwrap().status, Status::TimedOut);
}

#[test]
fn 提交前最后取消立即阻断但临界区内取消必须等提交结果() {
    let now = Instant::now();
    let mut before = finalizing(now);
    before.verified(now, true).unwrap();
    before.cancel(now).unwrap();
    assert!(before.begin_publish(now).is_err());
    assert_eq!(before.take_outcome().unwrap().status, Status::Cancelled);
    let mut during = publishing(now);
    during.cancel(now).unwrap();
    during.checkpoint(now + Duration::from_secs(120)).unwrap();
    assert!(during.take_outcome().is_none());
    assert!(!during.can_cleanup());
    during.committed(true, false, false).unwrap();
    assert_eq!(
        during.take_outcome(),
        Some(Outcome {
            status: Status::Succeeded,
            output: Output::Published {
                directory_synced: false,
                cleanup_complete: false
            }
        })
    );
}

#[test]
fn 临界区确认未提交后才可报告取消超时或提交失败() {
    let now = Instant::now();
    for cause in 0..3 {
        let mut task = publishing(now);
        let end = if cause == 1 {
            now + Duration::from_secs(120)
        } else {
            now
        };
        if cause == 0 {
            task.cancel(now).unwrap();
        }
        task.publication_failed(end).unwrap();
        assert_eq!(
            task.take_outcome().unwrap().status,
            match cause {
                0 => Status::Cancelled,
                1 => Status::TimedOut,
                _ => Status::Failed(Failure::Publication),
            }
        );
        assert!(task.can_cleanup());
    }
}

#[test]
fn 提交回执未知与位置不确定均不能假报成功() {
    let now = Instant::now();
    let mut unknown = publishing(now);
    unknown.commit_unconfirmed().unwrap();
    assert_eq!(
        unknown.take_outcome(),
        Some(Outcome {
            status: Status::Unknown,
            output: Output::Uncertain
        })
    );
    assert!(!unknown.can_cleanup());
    let mut moved = publishing(now);
    moved.committed(false, true, false).unwrap();
    assert_eq!(
        moved.take_outcome(),
        Some(Outcome {
            status: Status::Unknown,
            output: Output::Published {
                directory_synced: true,
                cleanup_complete: false
            }
        })
    );
}

#[test]
fn 一次任务最多取一个终态且重复回调不能复活() {
    let now = Instant::now();
    let mut task = publishing(now);
    task.committed(true, true, true).unwrap();
    assert_eq!(task.take_outcome().unwrap().status, Status::Succeeded);
    assert!(task.committed(true, true, true).is_err());
    assert!(task.publication_failed(now).is_err());
    assert!(task.protocol_failed(now).is_err());
    assert!(task.tree_stopped(now).is_err());
    assert!(task.ready(now, PLAN, evidence()).is_err());
    task.cancel(now + Duration::from_secs(130)).unwrap();
    assert_eq!(task.phase(), Phase::Finished);
    assert!(task.take_outcome().is_none());
}

#[test]
fn 就绪后重复结果或额外进度使候选失效() {
    let now = Instant::now();
    for duplicate in [false, true] {
        let mut task = running(now);
        task.ready(now, PLAN, evidence()).unwrap();
        if duplicate {
            assert!(task.ready(now, PLAN, evidence()).is_err());
        } else {
            assert!(task.progress(now, Stage::Verify).is_err());
        }
        assert_eq!(task.stop_reason(), Some(StopReason::Failure));
        task.tree_stopped(now).unwrap();
        assert!(task.artifact().is_none());
        assert_eq!(
            task.take_outcome().unwrap().status,
            Status::Failed(Failure::Protocol)
        );
    }
}

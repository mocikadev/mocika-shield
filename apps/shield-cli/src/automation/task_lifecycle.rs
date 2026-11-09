//! 父进程拥有的单任务状态规则；不启动进程、不读写文件，不把工作进程消息当停止证明。
use serde::{Deserialize, Serialize};
use shield_core::diagnostic::FailureCode;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Stage {
    Prepare,
    CheckTools,
    Preflight,
    ValidateResources,
    Unpack,
    ModifyManifest,
    ProcessDex,
    InjectRuntime,
    Repack,
    Align,
    Sign,
    Verify,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ArtifactEvidence {
    pub bytes: u64,
    pub sha256: [u8; 32],
    pub certificate_sha256: [u8; 32],
    pub signature_checked: bool,
    pub protection_checked: bool,
    pub alignment_checked: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content_sha256: Option<[u8; 32]>,
}

impl ArtifactEvidence {
    fn valid(&self, certificate: [u8; 32], content: Option<[u8; 32]>) -> bool {
        self.bytes > 0
            && self.certificate_sha256 == certificate
            && self.signature_checked
            && match content {
                None => self.protection_checked && self.content_sha256.is_none(),
                Some(expected) => !self.protection_checked && self.content_sha256 == Some(expected),
            }
            && self.alignment_checked
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum StopReason {
    Cancel,
    Timeout,
    Failure,
    Finalize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Phase {
    Starting,
    Handshaken,
    Running,
    Stopping,
    Finalizing,
    ReadyToPublish,
    Publishing,
    Finished,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Failure {
    Worker(FailureCode),
    Protocol,
    InvalidEvidence,
    Verification,
    UnexpectedExit,
    Publication,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Status {
    Succeeded,
    Failed(Failure),
    Cancelled,
    TimedOut,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Output {
    NotPublished,
    Published {
        directory_synced: bool,
        cleanup_complete: bool,
    },
    Uncertain,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Outcome {
    pub status: Status,
    pub output: Output,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TransitionError {
    InvalidTime,
    InvalidTransition,
    Interrupted,
}

#[derive(Clone, Copy)]
enum Goal {
    Candidate,
    Cancel,
    Timeout,
    Failed(Failure),
}

pub(super) struct Task {
    deadline: Instant,
    last_now: Instant,
    phase: Phase,
    isolated: bool,
    tree_stopped: bool,
    expected_plan: [u8; 32],
    expected_certificate: [u8; 32],
    expected_content: Option<[u8; 32]>,
    candidate: Option<ArtifactEvidence>,
    stage: Option<Stage>,
    goal: Option<Goal>,
    deferred: Option<Goal>,
    outcome: Option<Outcome>,
    outcome_taken: bool,
}

impl Task {
    pub fn new(
        now: Instant,
        timeout: Duration,
        plan: [u8; 32],
        certificate: [u8; 32],
    ) -> Result<Self, TransitionError> {
        let deadline = now
            .checked_add(timeout)
            .filter(|_| !timeout.is_zero())
            .ok_or(TransitionError::InvalidTime)?;
        Ok(Self {
            deadline,
            last_now: now,
            phase: Phase::Starting,
            isolated: false,
            tree_stopped: false,
            expected_plan: plan,
            expected_certificate: certificate,
            expected_content: None,
            candidate: None,
            stage: None,
            goal: None,
            deferred: None,
            outcome: None,
            outcome_taken: false,
        })
    }

    pub fn phase(&self) -> Phase {
        self.phase
    }

    /// 只有父端创建任务时能选择签名验收，工作进程的字段不能改变任务种类。
    #[cfg(any(test, all(feature = "agent-executor", target_os = "macos")))]
    pub fn new_sign(
        now: Instant,
        timeout: Duration,
        plan: [u8; 32],
        certificate: [u8; 32],
        content: [u8; 32],
    ) -> Result<Self, TransitionError> {
        let mut task = Self::new(now, timeout, plan, certificate)?;
        task.expected_content = Some(content);
        Ok(task)
    }

    /// 必须由平台进程控制层确认；工作进程不能通过消息设置此标志。
    pub fn isolation_confirmed(&mut self, now: Instant) -> Result<(), TransitionError> {
        self.active(now)?;
        if !matches!(self.phase, Phase::Starting | Phase::Handshaken) || self.isolated {
            return Err(TransitionError::InvalidTransition);
        }
        self.isolated = true;
        Ok(())
    }

    pub fn hello(&mut self, now: Instant) -> Result<(), TransitionError> {
        self.worker_active(now)?;
        if self.phase != Phase::Starting {
            return self.violation();
        }
        self.phase = Phase::Handshaken;
        Ok(())
    }

    pub fn start(&mut self, now: Instant) -> Result<(), TransitionError> {
        self.active(now)?;
        if self.phase != Phase::Handshaken || !self.isolated {
            return Err(TransitionError::InvalidTransition);
        }
        self.phase = Phase::Running;
        Ok(())
    }

    pub fn progress(&mut self, now: Instant, stage: Stage) -> Result<(), TransitionError> {
        self.worker_active(now)?;
        if self.phase != Phase::Running || self.stage.is_some_and(|previous| stage <= previous) {
            return self.violation();
        }
        self.stage = Some(stage);
        Ok(())
    }

    pub fn ready(
        &mut self,
        now: Instant,
        plan: [u8; 32],
        artifact: ArtifactEvidence,
    ) -> Result<(), TransitionError> {
        self.worker_active(now)?;
        if self.phase != Phase::Running {
            return self.violation();
        }
        if plan != self.expected_plan
            || !artifact.valid(self.expected_certificate, self.expected_content)
        {
            self.stop(Goal::Failed(Failure::InvalidEvidence));
            return Err(TransitionError::InvalidTransition);
        }
        self.candidate = Some(artifact);
        self.stop(Goal::Candidate);
        Ok(())
    }

    pub fn worker_failed(
        &mut self,
        now: Instant,
        code: FailureCode,
    ) -> Result<(), TransitionError> {
        self.worker_active(now)?;
        if self.phase != Phase::Running {
            return self.violation();
        }
        self.stop(Goal::Failed(Failure::Worker(code)));
        Ok(())
    }

    /// 父端启动前复核失败，不冒充已经运行的工作进程错误；仍须确认整组停止。
    pub fn preparation_failed(&mut self, now: Instant) -> Result<(), TransitionError> {
        self.active(now)?;
        if !matches!(self.phase, Phase::Starting | Phase::Handshaken) {
            return Err(TransitionError::InvalidTransition);
        }
        self.stop(Goal::Failed(Failure::Verification));
        Ok(())
    }

    pub fn protocol_failed(&mut self, now: Instant) -> Result<(), TransitionError> {
        self.observe(now)?;
        if matches!(self.phase, Phase::Finished | Phase::Publishing) {
            return Err(TransitionError::InvalidTransition);
        }
        self.stop(Goal::Failed(Failure::Protocol));
        Ok(())
    }

    pub fn channel_closed(&mut self, now: Instant) -> Result<(), TransitionError> {
        self.observe(now)?;
        if self.phase == Phase::Stopping {
            return Ok(());
        }
        self.active(now)?;
        if !matches!(
            self.phase,
            Phase::Starting | Phase::Handshaken | Phase::Running
        ) {
            return Err(TransitionError::InvalidTransition);
        }
        self.stop(Goal::Failed(Failure::UnexpectedExit));
        Ok(())
    }

    pub fn cancel(&mut self, now: Instant) -> Result<(), TransitionError> {
        self.observe(now)?;
        if self.phase != Phase::Finished {
            self.stop(Goal::Cancel);
        }
        Ok(())
    }

    pub fn checkpoint(&mut self, now: Instant) -> Result<(), TransitionError> {
        self.active(now)
    }

    /// 停止目标只用于驱动平台层，不能当作进程树已经停止的证明。
    pub fn stop_reason(&self) -> Option<StopReason> {
        if self.phase != Phase::Stopping {
            return None;
        }
        self.goal.map(|goal| match goal {
            Goal::Candidate => StopReason::Finalize,
            Goal::Cancel => StopReason::Cancel,
            Goal::Timeout => StopReason::Timeout,
            Goal::Failed(_) => StopReason::Failure,
        })
    }

    /// 只能由父进程平台层在确认整组已停止后调用；EOF、退出码或 Ready 不能替代。
    pub fn tree_stopped(&mut self, now: Instant) -> Result<(), TransitionError> {
        self.observe(now)?;
        if matches!(self.phase, Phase::Finished | Phase::Publishing) || self.tree_stopped {
            return Err(TransitionError::InvalidTransition);
        }
        self.tree_stopped = true;
        match self.goal {
            Some(Goal::Candidate) => self.phase = Phase::Finalizing,
            Some(goal) => self.finish_goal(goal),
            None => self.finish_goal(Goal::Failed(Failure::UnexpectedExit)),
        }
        Ok(())
    }

    pub fn stop_unconfirmed(&mut self, now: Instant) -> Result<(), TransitionError> {
        self.observe(now)?;
        if self.phase != Phase::Stopping {
            return Err(TransitionError::InvalidTransition);
        }
        self.finish(Status::Unknown, Output::NotPublished);
        Ok(())
    }

    pub fn verified(&mut self, now: Instant, valid: bool) -> Result<(), TransitionError> {
        self.active(now)?;
        if self.phase != Phase::Finalizing || !self.tree_stopped {
            return Err(TransitionError::InvalidTransition);
        }
        if valid {
            self.phase = Phase::ReadyToPublish;
        } else {
            self.finish_goal(Goal::Failed(Failure::Verification));
        }
        Ok(())
    }

    pub fn artifact(&self) -> Option<ArtifactEvidence> {
        matches!(
            self.phase,
            Phase::Finalizing | Phase::ReadyToPublish | Phase::Publishing
        )
        .then_some(self.candidate)
        .flatten()
    }

    /// 只在文件基础层的最后提交检查点进入此状态，复制阶段仍保持可取消。
    pub fn begin_publish(&mut self, now: Instant) -> Result<(), TransitionError> {
        self.active(now)?;
        if self.phase != Phase::ReadyToPublish || !self.tree_stopped {
            return Err(TransitionError::InvalidTransition);
        }
        self.phase = Phase::Publishing;
        Ok(())
    }

    pub fn committed(
        &mut self,
        location_confirmed: bool,
        directory_synced: bool,
        cleanup_complete: bool,
    ) -> Result<(), TransitionError> {
        if self.phase != Phase::Publishing {
            return Err(TransitionError::InvalidTransition);
        }
        self.finish(
            if location_confirmed {
                Status::Succeeded
            } else {
                Status::Unknown
            },
            Output::Published {
                directory_synced,
                cleanup_complete,
            },
        );
        Ok(())
    }

    pub fn publication_failed(&mut self, now: Instant) -> Result<(), TransitionError> {
        self.observe(now)?;
        if !matches!(self.phase, Phase::ReadyToPublish | Phase::Publishing) || !self.tree_stopped {
            return Err(TransitionError::InvalidTransition);
        }
        self.finish_goal(self.deferred.unwrap_or(Goal::Failed(Failure::Publication)));
        Ok(())
    }

    pub fn commit_unconfirmed(&mut self) -> Result<(), TransitionError> {
        if self.phase != Phase::Publishing {
            return Err(TransitionError::InvalidTransition);
        }
        self.finish(Status::Unknown, Output::Uncertain);
        Ok(())
    }

    pub fn can_cleanup(&self) -> bool {
        self.tree_stopped
            && self
                .outcome
                .is_some_and(|outcome| outcome.output != Output::Uncertain)
    }

    pub fn take_outcome(&mut self) -> Option<Outcome> {
        if self.outcome_taken {
            return None;
        }
        let outcome = self.outcome?;
        self.outcome_taken = true;
        Some(outcome)
    }

    fn observe(&mut self, now: Instant) -> Result<(), TransitionError> {
        if now < self.last_now {
            return Err(TransitionError::InvalidTime);
        }
        self.last_now = now;
        if now >= self.deadline && self.phase != Phase::Finished {
            self.stop(Goal::Timeout);
        }
        Ok(())
    }

    fn active(&mut self, now: Instant) -> Result<(), TransitionError> {
        self.observe(now)?;
        if matches!(self.phase, Phase::Stopping | Phase::Finished) {
            return Err(TransitionError::Interrupted);
        }
        Ok(())
    }

    fn worker_active(&mut self, now: Instant) -> Result<(), TransitionError> {
        self.observe(now)?;
        if self.phase == Phase::Stopping && matches!(self.goal, Some(Goal::Candidate)) {
            // Ready 后再出现结果或进度，说明流未正常结束，不能继续验收该候选。
            return self.violation();
        }
        if matches!(self.phase, Phase::Stopping | Phase::Finished) {
            return Err(TransitionError::Interrupted);
        }
        Ok(())
    }

    fn violation(&mut self) -> Result<(), TransitionError> {
        self.stop(Goal::Failed(Failure::Protocol));
        Err(TransitionError::InvalidTransition)
    }

    fn stop(&mut self, goal: Goal) {
        if self.phase == Phase::Publishing {
            if self.deferred.is_none() {
                self.deferred = Some(goal);
            }
        } else if self.phase != Phase::Finished {
            // 故障不能被后来的取消遮盖；取消和超时保留先观察到的原因。
            if !matches!(self.goal, Some(Goal::Failed(_)))
                && (matches!(goal, Goal::Failed(_))
                    || matches!(self.goal, None | Some(Goal::Candidate)))
            {
                self.goal = Some(goal);
            }
            if self.tree_stopped {
                self.finish_goal(self.goal.unwrap_or(goal));
            } else {
                self.phase = Phase::Stopping;
            }
        }
    }

    fn finish_goal(&mut self, goal: Goal) {
        self.finish(
            match goal {
                Goal::Cancel => Status::Cancelled,
                Goal::Timeout => Status::TimedOut,
                Goal::Failed(failure) => Status::Failed(failure),
                Goal::Candidate => Status::Unknown,
            },
            Output::NotPublished,
        );
    }

    fn finish(&mut self, status: Status, output: Output) {
        self.phase = Phase::Finished;
        self.outcome = Some(Outcome { status, output });
    }
}

#[cfg(test)]
#[path = "task_lifecycle_tests.rs"]
mod tests;

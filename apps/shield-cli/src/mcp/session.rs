//! 会话内有限计划及执行证据；不启动进程、不保存凭据、不负责最终文件提交。
use super::policy::Result;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::io::Read;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const CAPACITY: usize = 16;
const TTL: Duration = Duration::from_secs(600);

enum Phase {
    Planned,
    Running(Option<&'static str>),
    Finished { result: Value, ended: Instant },
}
struct Record {
    arguments: Value,
    binding: Option<String>,
    digest: String,
    created: Instant,
    phase: Phase,
}
#[derive(Default)]
pub(super) struct Session {
    records: HashMap<String, Record>,
    execution_blocked: bool,
}
impl Session {
    pub fn block_execution(&mut self) {
        self.execution_blocked = true;
    }
    pub fn execution_blocked(&self) -> bool {
        self.execution_blocked
    }
    fn prune(&mut self, now: Instant) {
        self.records.retain(|_, record| match record.phase {
            Phase::Planned => now.saturating_duration_since(record.created) < TTL,
            Phase::Running(_) => true,
            Phase::Finished { ended, .. } => now.saturating_duration_since(ended) < TTL,
        });
    }
    pub fn can_plan(&mut self) -> Result<()> {
        if self.execution_blocked {
            return Err("SESSION_RECOVERY_REQUIRED");
        }
        self.prune(Instant::now());
        if self.records.len() < CAPACITY {
            Ok(())
        } else {
            Err("PLAN_CAPACITY_REACHED")
        }
    }
    #[cfg(test)]
    pub fn insert(&mut self, arguments: Value, digest: String) -> Result<String> {
        self.insert_bound(arguments, digest, None)
    }
    pub fn insert_bound(
        &mut self,
        arguments: Value,
        digest: String,
        binding: Option<String>,
    ) -> Result<String> {
        self.can_plan()?;
        for _ in 0..4 {
            let mut random = [0u8; 16];
            std::fs::File::open("/dev/urandom")
                .and_then(|mut file| file.read_exact(&mut random))
                .map_err(|_| "PLAN_ID_UNAVAILABLE")?;
            let id: String = random.iter().map(|byte| format!("{byte:02x}")).collect();
            if !self.records.contains_key(&id) {
                self.records.insert(
                    id.clone(),
                    Record {
                        arguments,
                        binding,
                        digest,
                        created: Instant::now(),
                        phase: Phase::Planned,
                    },
                );
                return Ok(id);
            }
        }
        Err("PLAN_ID_UNAVAILABLE")
    }
    pub fn begin(&mut self, id: &str, digest: &str) -> Result<(Value, Option<String>)> {
        self.prune(Instant::now());
        let record = self.records.get_mut(id).ok_or("PLAN_NOT_AVAILABLE")?;
        if !matches!(record.phase, Phase::Planned) {
            return Err("PLAN_ALREADY_CONSUMED");
        }
        if self.execution_blocked {
            return Err("SESSION_RECOVERY_REQUIRED");
        }
        if !record.digest.eq_ignore_ascii_case(digest) {
            return Err("PLAN_DIGEST_MISMATCH");
        }
        record.phase = Phase::Running(None);
        Ok((record.arguments.clone(), record.binding.clone()))
    }
    pub fn invalidate_plans(&mut self) {
        self.records
            .retain(|_, record| !matches!(record.phase, Phase::Planned));
    }
    pub fn progress(&mut self, id: &str, stage: &'static str) {
        if let Some(Record {
            phase: Phase::Running(current),
            ..
        }) = self.records.get_mut(id)
        {
            *current = Some(stage);
        }
    }
    fn finish(&mut self, id: &str, result: Value) {
        if let Some(record) = self.records.get_mut(id) {
            if matches!(record.phase, Phase::Running(_)) {
                if result["status"] == "unknown"
                    || result["output_state"] == "uncertain"
                    || matches!(
                        result["cleanup_state"].as_str(),
                        Some("incomplete" | "retained" | "unconfirmed")
                    )
                {
                    self.execution_blocked = true;
                }
                record.phase = Phase::Finished {
                    result,
                    ended: Instant::now(),
                };
            }
        }
    }
    pub fn status(&mut self, id: &str) -> Result<Value> {
        self.prune(Instant::now());
        let record = self.records.get(id).ok_or("PLAN_NOT_AVAILABLE")?;
        let mut result = match &record.phase {
            Phase::Planned => {
                json!({"schema_version":1,"state":"planned","status":"planned","execution_ready":false})
            }
            Phase::Running(stage) => {
                json!({"schema_version":1,"state":"running","status":"running","stage":stage,
                "output_state":"uncertain","cleanup_state":"unconfirmed","retryable":false})
            }
            Phase::Finished { result, .. } => {
                let mut result = result.clone();
                result["state"] = json!("finished");
                result
            }
        };
        result["plan_id"] = json!(id);
        // 状态查询不会重新检查环境或授予执行权限，不把缓存状态当成新计划。
        result["live_policy_checked"] = json!(false);
        result["session_execution_blocked"] = json!(self.execution_blocked);
        Ok(result)
    }
}

pub(super) type Shared = Arc<Mutex<Session>>;
pub(super) fn lock(session: &Shared) -> std::sync::MutexGuard<'_, Session> {
    session
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}
pub(super) struct Attempt {
    session: Shared,
    id: String,
    finished: bool,
}
impl Attempt {
    pub fn new(session: Shared, id: String) -> Self {
        Self {
            session,
            id,
            finished: false,
        }
    }
    pub fn finish(&mut self, result: Value) {
        lock(&self.session).finish(&self.id, result);
        self.finished = true;
    }
}
impl Drop for Attempt {
    fn drop(&mut self) {
        if !self.finished {
            lock(&self.session).finish(&self.id, unknown("TASK_STATE_UNKNOWN"));
        }
    }
}
pub(super) fn unknown(code: &'static str) -> Value {
    json!({"schema_version":1,"operation":"agent_protect","status":"unknown","code":code,
        "output_state":"uncertain","cleanup_state":"unconfirmed","retryable":false})
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn 共享绑定只随同一次消费返回内部执行不出现在状态中() {
        let mut session = Session::default();
        let id = session
            .insert_bound(
                json!({"certificate_id":"allowed"}),
                "a".repeat(64),
                Some("private-binding".into()),
            )
            .unwrap();
        assert!(!session
            .status(&id)
            .unwrap()
            .to_string()
            .contains("private-binding"));
        assert!(session.begin(&id, &"b".repeat(64)).is_err());
        let (args, binding) = session.begin(&id, &"a".repeat(64)).unwrap();
        assert_eq!(args["certificate_id"], "allowed");
        assert_eq!(binding.as_deref(), Some("private-binding"));
        assert!(session.begin(&id, &"a".repeat(64)).is_err());
        assert!(!session
            .status(&id)
            .unwrap()
            .to_string()
            .contains("private-binding"));
    }
    #[test]
    fn 有限计划限时单次消费且状态不会复活执行权限() {
        let mut session = Session::default();
        let id = session
            .insert(json!({"input":"a.apk"}), "a".repeat(64))
            .unwrap();
        assert!(session.begin(&id, &"b".repeat(64)).is_err());
        assert!(session.begin(&id, &"a".repeat(64)).is_ok());
        assert!(session.begin(&id, &"a".repeat(64)).is_err());
        session.records.get_mut(&id).unwrap().created = Instant::now() - TTL;
        assert_eq!(session.status(&id).unwrap()["state"], "running");
        session.finish(
            &id,
            json!({"status":"failed","output_state":"not_published","cleanup_state":"complete"}),
        );
        assert_eq!(session.status(&id).unwrap()["status"], "failed");
        assert!(session.begin(&id, &"a".repeat(64)).is_err());
        for _ in 1..CAPACITY {
            session.insert(json!({}), "a".repeat(64)).unwrap();
        }
        assert_eq!(session.can_plan(), Err("PLAN_CAPACITY_REACHED"));
        session.invalidate_plans();
        assert_eq!(session.records.len(), 1);
        assert!(Session::default().status(&id).is_err());
    }
    #[test]
    fn 到期计划和异常退出的尝试不会变为可重试任务() {
        let shared = Arc::new(Mutex::new(Session::default()));
        let id = lock(&shared).insert(json!({}), "a".repeat(64)).unwrap();
        lock(&shared).records.get_mut(&id).unwrap().created = Instant::now() - TTL;
        assert!(lock(&shared).begin(&id, &"a".repeat(64)).is_err());
        let id = lock(&shared).insert(json!({}), "a".repeat(64)).unwrap();
        let pending = lock(&shared).insert(json!({}), "b".repeat(64)).unwrap();
        lock(&shared).begin(&id, &"a".repeat(64)).unwrap();
        drop(Attempt::new(shared.clone(), id.clone()));
        assert_eq!(lock(&shared).status(&id).unwrap()["status"], "unknown");
        assert_eq!(lock(&shared).can_plan(), Err("SESSION_RECOVERY_REQUIRED"));
        assert_eq!(
            lock(&shared).status(&id).unwrap()["session_execution_blocked"],
            true
        );
        assert_eq!(
            lock(&shared).begin(&pending, &"b".repeat(64)),
            Err("SESSION_RECOVERY_REQUIRED")
        );
        if let Phase::Finished { ended, .. } =
            &mut lock(&shared).records.get_mut(&id).unwrap().phase
        {
            *ended = Instant::now() - TTL;
        }
        assert_eq!(lock(&shared).can_plan(), Err("SESSION_RECOVERY_REQUIRED"));
    }
}

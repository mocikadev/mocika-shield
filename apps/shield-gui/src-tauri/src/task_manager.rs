use serde::Serialize;
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::{Emitter, Window};

const MAX_LOGS: usize = 200;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum TaskKind {
    Protect,
    Sign,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum TaskStatus {
    Running,
    Succeeded,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct TaskLog {
    pub(crate) timestamp_ms: u64,
    pub(crate) step: String,
    pub(crate) level: &'static str,
    pub(crate) message: String,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct TaskSnapshot {
    pub(crate) task_id: String,
    pub(crate) kind: TaskKind,
    pub(crate) status: TaskStatus,
    pub(crate) current_step: String,
    pub(crate) input_path: String,
    pub(crate) output_path: String,
    pub(crate) started_at_ms: u64,
    pub(crate) finished_at_ms: Option<u64>,
    pub(crate) logs: Vec<TaskLog>,
    pub(crate) error: Option<String>,
}

#[derive(Clone, Default)]
pub(crate) struct TaskManager(Arc<Mutex<BTreeMap<String, TaskSnapshot>>>);

impl TaskManager {
    pub(crate) fn begin(
        &self,
        window: &Window,
        task_id: String,
        kind: TaskKind,
        input_path: String,
        output_path: String,
        first_step: &str,
    ) -> Result<(), String> {
        let mut tasks = self.0.lock().map_err(|_| "任务状态锁已损坏".to_string())?;
        if tasks
            .values()
            .any(|task| task.kind == kind && task.status == TaskStatus::Running)
        {
            return Err(match kind {
                TaskKind::Protect => "已有加固任务正在执行".to_string(),
                TaskKind::Sign => "已有签名任务正在执行".to_string(),
            });
        }
        let now = now_ms();
        let snapshot = TaskSnapshot {
            task_id: task_id.clone(),
            kind,
            status: TaskStatus::Running,
            current_step: first_step.to_string(),
            input_path,
            output_path,
            started_at_ms: now,
            finished_at_ms: None,
            logs: vec![TaskLog {
                timestamp_ms: now,
                step: first_step.to_string(),
                level: "info",
                message: "任务已开始".to_string(),
            }],
            error: None,
        };
        tasks.insert(task_id, snapshot.clone());
        drop(tasks);
        emit_snapshot(window, &snapshot)
    }

    pub(crate) fn progress(
        &self,
        window: &Window,
        task_id: &str,
        step: &str,
        message: impl Into<String>,
    ) -> Result<(), String> {
        let mut tasks = self.0.lock().map_err(|_| "任务状态锁已损坏".to_string())?;
        let task = tasks
            .get_mut(task_id)
            .ok_or_else(|| "未找到任务状态".to_string())?;
        task.current_step = step.to_string();
        task.logs.push(TaskLog {
            timestamp_ms: now_ms(),
            step: step.to_string(),
            level: "info",
            message: message.into(),
        });
        if task.logs.len() > MAX_LOGS {
            task.logs.drain(..task.logs.len() - MAX_LOGS);
        }
        let snapshot = task.clone();
        drop(tasks);
        emit_snapshot(window, &snapshot)
    }

    pub(crate) fn finish(
        &self,
        window: &Window,
        task_id: &str,
        status: TaskStatus,
        error: Option<String>,
    ) -> Result<(), String> {
        let mut tasks = self.0.lock().map_err(|_| "任务状态锁已损坏".to_string())?;
        let task = tasks
            .get_mut(task_id)
            .ok_or_else(|| "未找到任务状态".to_string())?;
        let now = now_ms();
        task.status = status;
        task.finished_at_ms = Some(now);
        task.error = error.clone();
        task.logs.push(TaskLog {
            timestamp_ms: now,
            step: task.current_step.clone(),
            level: if status == TaskStatus::Failed {
                "error"
            } else {
                "info"
            },
            message: error.unwrap_or_else(|| match status {
                TaskStatus::Succeeded => "任务已完成".to_string(),
                TaskStatus::Cancelled => "任务已取消".to_string(),
                _ => "任务已结束".to_string(),
            }),
        });
        let snapshot = task.clone();
        drop(tasks);
        emit_snapshot(window, &snapshot)
    }

    pub(crate) fn latest(&self, kind: TaskKind) -> Result<Option<TaskSnapshot>, String> {
        let tasks = self.0.lock().map_err(|_| "任务状态锁已损坏".to_string())?;
        Ok(tasks
            .values()
            .filter(|task| task.kind == kind)
            .max_by_key(|task| task.started_at_ms)
            .cloned())
    }

    pub(crate) fn snapshot(&self, task_id: &str) -> Result<Option<TaskSnapshot>, String> {
        let tasks = self.0.lock().map_err(|_| "任务状态锁已损坏".to_string())?;
        Ok(tasks.get(task_id).cloned())
    }
}

fn emit_snapshot(window: &Window, snapshot: &TaskSnapshot) -> Result<(), String> {
    window
        .emit("task-state", snapshot)
        .map_err(|err| format!("发送任务状态失败: {err}"))
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[cfg(test)]
mod tests {
    use super::{TaskKind, TaskStatus};

    #[test]
    fn task_kind_和状态保持稳定序列化值() {
        assert_eq!(
            serde_json::to_string(&TaskKind::Protect).unwrap(),
            "\"protect\""
        );
        assert_eq!(
            serde_json::to_string(&TaskStatus::Succeeded).unwrap(),
            "\"succeeded\""
        );
    }
}

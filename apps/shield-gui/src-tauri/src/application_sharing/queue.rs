//! 同一锁串行化授权读取、撤销、重新开启和发送决策。始终先取此锁，再访问配置。
use super::{
    application_sharing_enabled, protocol::Submission, set_application_sharing_preference,
};
use crate::app_config::AppConfigState;
#[cfg(test)]
use crate::application_identity::ApplicationIdentity;
use std::{
    collections::{HashMap, VecDeque},
    sync::{Arc, Mutex},
};

#[derive(Clone)]
pub(super) struct Pending {
    pub(super) payload: Submission,
    generation: u64,
    created: u64,
    due: u64,
    retried: bool,
    in_flight: bool,
}

#[derive(Default)]
pub(super) struct Inner {
    generations: HashMap<String, u64>,
    queue: VecDeque<Pending>,
    pub(super) running: bool,
}

#[derive(Clone, Default)]
pub(crate) struct SharingState {
    pub(super) inner: Arc<Mutex<Inner>>,
    pub(super) inspections: Arc<Mutex<super::inspection::InspectionStore>>,
}

impl SharingState {
    pub(super) fn suppress(&self, config: &AppConfigState, package: &str) {
        let Ok(mut state) = self.inner.lock() else {
            return;
        };
        *state.generations.entry(package.to_string()).or_default() += 1;
        state
            .queue
            .retain(|item| item.payload.package_name != package);
        // 清理不是用户永久退出：只锁住本会话，不修改磁盘偏好。
        // 锁损坏时授权查询自身也会失败关闭。
        let _ = config.block_application_sharing(package);
    }

    #[cfg(test)]
    pub(super) fn generation(&self, package: &str) -> u64 {
        self.inner
            .lock()
            .map(|state| *state.generations.get(package).unwrap_or(&0))
            .unwrap_or(u64::MAX)
    }

    pub(super) fn preference(
        &self,
        config: &AppConfigState,
        package: &str,
        enabled: bool,
    ) -> Result<(), String> {
        let mut state = self
            .inner
            .lock()
            .map_err(|_| "应用分享状态不可用".to_string())?;
        // 每次修改都使旧快照失效；保存失败同样不得恢复旧任务。
        *state.generations.entry(package.to_string()).or_default() += 1;
        state
            .queue
            .retain(|item| item.payload.package_name != package);
        set_application_sharing_preference(config, package, enabled)
    }

    pub(super) fn authorization(&self, config: &AppConfigState, package: &str) -> Option<u64> {
        let state = self.inner.lock().ok()?;
        application_sharing_enabled(config, package)
            .ok()?
            .then(|| *state.generations.get(package).unwrap_or(&0))
    }

    #[cfg(test)]
    pub(super) fn enqueue(
        &self,
        config: &AppConfigState,
        identity: ApplicationIdentity,
        generation: u64,
        flow: &str,
        date: String,
        now: u64,
    ) {
        self.enqueue_payload(
            config,
            Submission::new(identity, flow, date, env!("CARGO_PKG_VERSION").into()),
            generation,
            now,
        );
    }

    pub(super) fn enqueue_payload(
        &self,
        config: &AppConfigState,
        payload: Submission,
        generation: u64,
        now: u64,
    ) {
        let Ok(mut state) = self.inner.lock() else {
            return;
        };
        state
            .queue
            .retain(|item| now.saturating_sub(item.created) < 300);
        if state.queue.len() >= 32
            || *state.generations.get(&payload.package_name).unwrap_or(&0) != generation
            || !application_sharing_enabled(config, &payload.package_name).unwrap_or(false)
            || serde_json::to_vec(&payload).map_or(true, |bytes| bytes.len() > 8192)
        {
            return;
        }
        state.queue.push_back(Pending {
            payload,
            generation,
            created: now,
            due: now,
            retried: false,
            in_flight: false,
        });
    }

    pub(super) fn next(&self, config: &AppConfigState, now: u64) -> Option<Pending> {
        let mut state = self.inner.lock().ok()?;
        state.queue.retain(|item| {
            now.saturating_sub(item.created) < 300
                && application_sharing_enabled(config, &item.payload.package_name).unwrap_or(false)
        });
        let item = state
            .queue
            .iter_mut()
            .find(|item| !item.in_flight && item.due <= now)?;
        item.in_flight = true;
        Some(item.clone())
    }

    pub(super) fn sent(&self, item: &Pending, retryable: bool, now: u64) {
        let Ok(mut state) = self.inner.lock() else {
            return;
        };
        let Some(index) = state.queue.iter().position(|entry| {
            entry.payload.submission_id == item.payload.submission_id
                && entry.generation == item.generation
        }) else {
            return;
        };
        if retryable && !item.retried && now.saturating_sub(item.created) < 270 {
            let pending = &mut state.queue[index];
            pending.retried = true;
            pending.due = now + 30;
            pending.in_flight = false;
        } else {
            state.queue.remove(index);
        }
    }

    #[cfg(test)]
    pub(super) fn pending_count(&self) -> usize {
        self.inner.lock().unwrap().queue.len()
    }

    pub(super) fn next_delay(&self, now: u64) -> Option<u64> {
        let mut state = self.inner.lock().ok()?;
        let delay = state
            .queue
            .iter()
            .map(|item| item.due.saturating_sub(now))
            .min();
        if delay.is_none() {
            state.running = false;
        }
        delay
    }
}

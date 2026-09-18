use crate::app_config::AppConfigState;
pub(crate) mod commands;
mod inspection;
mod protocol;
mod queue;
mod transport;
pub(crate) use queue::SharingState;

#[derive(Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct SharingChoice {
    inspection_id: String,
    enabled: bool,
}

impl SharingState {
    pub(crate) fn begin(
        &self,
        config: &AppConfigState,
        task_id: &str,
        input: &str,
        choice: Option<&SharingChoice>,
        auto_sign: bool,
        sign: bool,
    ) {
        if !choice.is_some_and(|choice| choice.enabled) {
            return;
        }
        let Ok(mut store) = self.inspections.lock() else {
            return;
        };
        let flow = if sign {
            "sign"
        } else if auto_sign {
            "protect_with_sign"
        } else {
            "protect"
        };
        if let Some(snapshot) = store.freeze(
            self,
            config,
            std::path::Path::new(input),
            choice,
            flow,
            protocol::now_seconds(),
        ) {
            store.tasks.insert(task_id.to_string(), snapshot);
        }
    }

    pub(crate) fn verify_input(&self, task_id: &str) {
        if let Ok(mut store) = self.inspections.lock() {
            if let Some(task) = store.tasks.get_mut(task_id) {
                task.verify_input();
            }
        }
    }

    pub(crate) fn protected(&self, task_id: &str) {
        if let Ok(mut store) = self.inspections.lock() {
            if let Some(task) = store.tasks.get_mut(task_id) {
                task.protected(protocol::now_seconds());
            }
        }
    }

    pub(crate) fn finish(
        &self,
        app: tauri::AppHandle,
        config: &AppConfigState,
        task_id: &str,
        kind: crate::task_manager::TaskKind,
        status: crate::task_manager::TaskStatus,
        protected: bool,
    ) {
        self.complete(
            config,
            task_id,
            kind,
            status,
            protected,
            protocol::now_seconds(),
        );
        transport::schedule(app, self.clone());
    }

    fn complete(
        &self,
        config: &AppConfigState,
        task_id: &str,
        kind: crate::task_manager::TaskKind,
        status: crate::task_manager::TaskStatus,
        protected: bool,
        now: u64,
    ) {
        let snapshot = self
            .inspections
            .lock()
            .ok()
            .and_then(|mut store| store.tasks.remove(task_id));
        if let Some(snapshot) = snapshot {
            let generation = snapshot.generation;
            if let Some(payload) = snapshot.submission(kind, status, protected, now) {
                self.enqueue_payload(config, payload, generation, now);
            }
        }
    }
}

#[cfg(test)]
mod runtime_tests;

pub(crate) fn application_sharing_enabled(
    state: &AppConfigState,
    package_name: &str,
) -> Result<bool, String> {
    state.application_sharing_enabled(package_name)
}

pub(crate) fn set_application_sharing_preference(
    state: &AppConfigState,
    package_name: &str,
    enabled: bool,
) -> Result<(), String> {
    let package_name = package_name.to_string();
    let persisted_package_name = package_name.clone();
    // 写盘完成前先失败关闭，避免重新开启的内存变更短暂成为发送许可。
    state.block_application_sharing(&package_name)?;
    let result = state.mutate(move |config| {
        config
            .application_sharing
            .preferences
            .insert(persisted_package_name, enabled);
    });
    match result {
        Ok(()) => state.clear_application_sharing_block(&package_name),
        Err(error) => Err(error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app_config::{AppConfig, AppConfigState};

    #[test]
    fn application_同包名取消后跨保存重读仍保持关闭且其他包名默认开启() {
        let dir = tempfile::tempdir().expect("应创建临时目录");
        let path = dir.path().join("config.toml");
        let state = AppConfigState::new(path.clone(), AppConfig::default());

        set_application_sharing_preference(&state, "com.example.shared", false)
            .expect("取消偏好应保存");

        let persisted: AppConfig =
            toml::from_str(&std::fs::read_to_string(path).expect("应读取配置"))
                .expect("配置应有效");
        let reloaded = AppConfigState::new(dir.path().join("reloaded.toml"), persisted);
        assert!(!application_sharing_enabled(&reloaded, "com.example.shared").expect("应读取偏好"));
        assert!(application_sharing_enabled(&reloaded, "com.example.other").expect("应读取偏好"));
    }

    #[test]
    fn application_偏好保存失败后当前会话仍保持关闭() {
        let dir = tempfile::tempdir().expect("应创建临时目录");
        let state = AppConfigState::new(dir.path().to_path_buf(), AppConfig::default());

        assert!(set_application_sharing_preference(&state, "com.example.failed", false).is_err());
        assert!(!application_sharing_enabled(&state, "com.example.failed").expect("应读取偏好"));
    }

    #[test]
    fn application_重新开启保存失败后当前会话仍禁止发送() {
        let dir = tempfile::tempdir().expect("应创建临时目录");
        let mut config = AppConfig::default();
        config
            .application_sharing
            .preferences
            .insert("com.example.failed".to_string(), false);
        let state = AppConfigState::new(dir.path().to_path_buf(), config);

        assert!(set_application_sharing_preference(&state, "com.example.failed", true).is_err());
        assert!(!application_sharing_enabled(&state, "com.example.failed").expect("应读取偏好"));
    }
}

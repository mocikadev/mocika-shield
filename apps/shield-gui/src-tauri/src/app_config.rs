use serde::{Deserialize, Serialize};
use shield_services::config::{ConfigError, ConfigStore, ProtectDefaultsPatch, Setting};
#[cfg(test)]
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

pub(crate) use shield_services::config::{normalize_protect_defaults, ProtectDefaults};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub(crate) struct UpdateCache {
    pub last_check: Option<i64>,
    pub latest_tag: Option<String>,
    pub release_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct TelemetryConfig {
    pub enabled: bool,
    pub anonymous_id: String,
    pub daily: std::collections::BTreeMap<String, DailyTelemetry>,
    /// 独立于旧分区；旧程序保存已知字段时不应归并这些来源快照。
    #[serde(default)]
    pub daily_sources: std::collections::BTreeMap<String, DailyTelemetry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub(crate) struct DailyTelemetry {
    /// 与 `daily` 的复合键冗余保存，避免迁移或上传时从键名反向解析。
    pub usage_date: String,
    /// 仅来自桌面端编译版本，不接受前端或 APK 输入。
    pub app_version: String,
    /// 事件记录时固定；旧记录为空，不在发送时套用当前身份。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub build_identity: Option<crate::build_identity::BuildIdentity>,
    pub app_start_count: u32,
    pub protect_start_count: u32,
    pub protect_success_count: u32,
    pub protect_failed_count: u32,
    pub sign_success_count: u32,
    pub sign_failed_count: u32,
    /// 固定失败阶段的聚合计数，不保存原始错误文本。
    pub failure_counts: std::collections::BTreeMap<String, u32>,
    pub failure_classifier_version: Option<u32>,
    pub failure_reason_counts: Vec<crate::failure_diagnostic::FailureReasonCount>,
    pub uploaded: bool,
}

impl Default for TelemetryConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            anonymous_id: uuid::Uuid::new_v4().to_string(),
            daily: std::collections::BTreeMap::new(),
            daily_sources: std::collections::BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct AppConfig {
    pub locale: String,
    pub theme_mode: String,
    pub dismissed_version: Option<String>,
    pub update_cache: UpdateCache,
    pub telemetry: TelemetryConfig,
    pub protect_defaults: ProtectDefaults,
    pub application_sharing: ApplicationSharingConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub(crate) struct ApplicationSharingConfig {
    pub preferences: std::collections::BTreeMap<String, bool>,
}

impl ApplicationSharingConfig {
    pub(crate) fn enabled_for(&self, package_name: &str) -> bool {
        self.preferences.get(package_name).copied().unwrap_or(true)
    }
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            locale: "zh".to_string(),
            theme_mode: "system".to_string(),
            dismissed_version: None,
            update_cache: UpdateCache::default(),
            telemetry: TelemetryConfig::default(),
            protect_defaults: ProtectDefaults::default(),
            application_sharing: ApplicationSharingConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct AppConfigPayload {
    pub locale: String,
    pub theme_mode: String,
    pub telemetry_enabled: bool,
    #[serde(default)]
    pub protect_defaults: Option<ProtectDefaults>,
}

#[derive(Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct AppConfigUpdate {
    pub locale: Setting<String>,
    pub theme_mode: Setting<String>,
    pub telemetry_enabled: Setting<bool>,
    pub protect_defaults: Setting<ProtectDefaultsPatch>,
}

impl From<&AppConfig> for AppConfigPayload {
    fn from(config: &AppConfig) -> Self {
        Self {
            locale: normalize_locale(&config.locale),
            theme_mode: normalize_theme_mode(&config.theme_mode),
            telemetry_enabled: config.telemetry.enabled,
            protect_defaults: Some(normalize_protect_defaults(&config.protect_defaults)),
        }
    }
}

pub(crate) struct AppConfigState {
    pub path: PathBuf,
    config: Mutex<AppConfig>,
    persisted: AtomicBool,
    application_sharing_blocked: Mutex<std::collections::BTreeSet<String>>,
    telemetry_identity: Option<crate::build_identity::BuildIdentity>,
}

impl AppConfigState {
    pub(crate) fn new(path: PathBuf, config: AppConfig) -> Self {
        let persisted = AtomicBool::new(path.is_file());
        Self {
            path,
            config: Mutex::new(config),
            persisted,
            application_sharing_blocked: Mutex::new(std::collections::BTreeSet::new()),
            telemetry_identity: None,
        }
    }

    pub(crate) fn with_telemetry_identity(
        mut self,
        identity: Option<crate::build_identity::BuildIdentity>,
    ) -> Self {
        self.telemetry_identity = identity;
        self
    }

    pub(crate) fn telemetry_identity(&self) -> Option<&crate::build_identity::BuildIdentity> {
        self.telemetry_identity.as_ref()
    }

    fn store(&self) -> ConfigStore {
        if self.persisted.load(Ordering::SeqCst) {
            ConfigStore::existing(self.path.clone())
        } else {
            ConfigStore::new(self.path.clone())
        }
    }

    pub(crate) fn read(&self) -> Result<AppConfig, String> {
        let initial = self
            .config
            .lock()
            .map_err(|_| "读取配置失败：配置状态锁已损坏".to_string())?
            .clone();
        let mut latest = self
            .store()
            .read(&initial)
            .map_err(|error| error.to_string())?;
        normalize_config(&mut latest);
        if self.path.is_file() {
            self.persisted.store(true, Ordering::SeqCst);
        }
        if let Ok(mut cached) = self.config.lock() {
            *cached = latest.clone();
        }
        Ok(latest)
    }

    pub(crate) fn mutate<F>(&self, mutator: F) -> Result<(), String>
    where
        F: FnOnce(&mut AppConfig),
    {
        self.mutate_checked(|config| {
            mutator(config);
            Ok(())
        })
    }
    fn mutate_checked(
        &self,
        mutator: impl FnOnce(&mut AppConfig) -> Result<(), ConfigError>,
    ) -> Result<(), String> {
        let initial = self
            .config
            .lock()
            .map_err(|_| "写入配置失败：配置状态锁已损坏".to_string())?
            .clone();
        // 不持有内存缓存锁等待跨进程文件锁；磁盘是更新基线，缓存只为首次缺文件提供种子。
        let latest = self
            .store()
            .update(&initial, |config| {
                mutator(config)?;
                normalize_config(config);
                Ok(())
            })
            .map_err(|error| error.to_string())?;
        self.persisted.store(true, Ordering::SeqCst);
        if let Ok(mut cached) = self.config.lock() {
            *cached = latest;
        }
        Ok(())
    }
    pub(crate) fn update_protect_defaults(
        &self,
        patch: ProtectDefaultsPatch,
    ) -> Result<(), String> {
        self.mutate_checked(|config| patch.apply(&mut config.protect_defaults))
    }
    pub(crate) fn update_preferences(&self, patch: AppConfigUpdate) -> Result<(), String> {
        self.mutate_checked(|config| {
            if let Setting::Set(locale) = patch.locale {
                if !["zh", "en"].contains(&locale.as_str()) {
                    return Err(ConfigError::InvalidValue);
                }
                config.locale = locale;
            }
            if let Setting::Set(theme) = patch.theme_mode {
                if !["light", "dark", "system"].contains(&theme.as_str()) {
                    return Err(ConfigError::InvalidValue);
                }
                config.theme_mode = theme;
            }
            if let Setting::Set(enabled) = patch.telemetry_enabled {
                config.telemetry.enabled = enabled;
            }
            if let Setting::Set(defaults) = patch.protect_defaults {
                defaults.apply(&mut config.protect_defaults)?;
            }
            Ok(())
        })
    }

    pub(crate) fn application_sharing_enabled(&self, package_name: &str) -> Result<bool, String> {
        // 本次会话的失败关闭先于磁盘读取，坏配置不能重新授权分享。
        if self
            .application_sharing_blocked
            .lock()
            .map_err(|_| "读取应用分享状态失败：状态锁已损坏".to_string())?
            .contains(package_name)
        {
            return Ok(false);
        }
        Ok(self.read()?.application_sharing.enabled_for(package_name))
    }

    pub(crate) fn block_application_sharing(&self, package_name: &str) -> Result<(), String> {
        self.application_sharing_blocked
            .lock()
            .map_err(|_| "写入应用分享状态失败：状态锁已损坏".to_string())?
            .insert(package_name.to_string());
        Ok(())
    }

    pub(crate) fn clear_application_sharing_block(&self, package_name: &str) -> Result<(), String> {
        self.application_sharing_blocked
            .lock()
            .map_err(|_| "写入应用分享状态失败：状态锁已损坏".to_string())?
            .remove(package_name);
        Ok(())
    }
}

#[derive(Debug)]
pub(crate) struct LoadedAppConfig {
    pub path: PathBuf,
    pub config: AppConfig,
}

pub(crate) fn normalize_locale(value: &str) -> String {
    if value.eq_ignore_ascii_case("en") {
        "en".to_string()
    } else {
        "zh".to_string()
    }
}

pub(crate) fn normalize_theme_mode(value: &str) -> String {
    match value {
        "light" | "dark" | "system" => value.to_string(),
        _ => "system".to_string(),
    }
}

pub(crate) use shield_services::certificates::normalize_keystore_type;

pub(crate) fn load_app_config(app: &tauri::AppHandle) -> Result<LoadedAppConfig, String> {
    let path = crate::app_paths::application_paths(app)?.config_file();
    let config = ConfigStore::new(path.clone())
        .initialize(&AppConfig::default(), |config| {
            normalize_config(config);
            Ok(())
        })
        .map_err(|error| error.to_string())?;
    Ok(LoadedAppConfig { path, config })
}

fn normalize_config(config: &mut AppConfig) {
    config.locale = normalize_locale(&config.locale);
    config.theme_mode = normalize_theme_mode(&config.theme_mode);
    config.protect_defaults = normalize_protect_defaults(&config.protect_defaults);
    config.dismissed_version = config
        .dismissed_version
        .take()
        .filter(|value| !value.trim().is_empty());
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Barrier};

    #[test]
    fn 外部业务更新可见且旧缓存保存不会覆盖未知字段() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let gui = AppConfigState::new(path.clone(), AppConfig::default());
        gui.mutate(|config| {
            config.telemetry.enabled = false;
            config
                .application_sharing
                .preferences
                .insert("com.example.app".into(), false);
        })
        .unwrap();
        // 模拟另一个入口已经写入新业务字段，GUI 仍持有旧缓存。
        let shared = ConfigStore::existing(path.clone());
        shared
            .update_protect_defaults(
                serde_json::from_str(
                    r#"{"runtime_mode":"android_api19","certificate_id":"other"}"#,
                )
                .unwrap(),
            )
            .unwrap();
        let mut doc: toml::Value = toml::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        doc.as_table_mut()
            .unwrap()
            .insert("future".into(), toml::Value::String("keep".into()));
        fs::write(&path, toml::to_string(&doc).unwrap()).unwrap();
        gui.update_preferences(serde_json::from_str(r#"{"locale":"en"}"#).unwrap())
            .unwrap();
        let current = gui.read().unwrap();
        assert_eq!(current.protect_defaults.runtime_mode, "android_api19");
        assert_eq!(
            current.protect_defaults.certificate_id.as_deref(),
            Some("other")
        );
        assert!(!current.telemetry.enabled);
        assert!(!current.application_sharing.enabled_for("com.example.app"));
        let doc: toml::Value = toml::from_str(&fs::read_to_string(path).unwrap()).unwrap();
        assert_eq!(doc["future"].as_str(), Some("keep"));
    }

    #[test]
    fn 配置损坏或已加载文件被删除时不能用缓存恢复() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let state = AppConfigState::new(path.clone(), AppConfig::default());
        state
            .mutate(|config| config.telemetry.enabled = false)
            .unwrap();
        fs::write(&path, "[broken").unwrap();
        assert!(state.read().is_err());
        assert!(state.mutate(|config| config.locale = "en".into()).is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), "[broken");
        fs::remove_file(&path).unwrap();
        assert!(state.read().is_err());
        assert!(state.mutate(|config| config.locale = "en".into()).is_err());
        assert!(!path.exists());
    }

    #[test]
    fn 空字段补齐不会重新同意统计或轮换已有匿名标识() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        fs::write(
            &path,
            "[telemetry]\nenabled=false\n[unknown]\nvalue='preserve'\n",
        )
        .unwrap();
        let store = ConfigStore::new(path.clone());
        let first = store
            .initialize(&AppConfig::default(), |config| {
                normalize_config(config);
                Ok(())
            })
            .unwrap();
        let before = fs::read(&path).unwrap();
        let again = store
            .initialize(&AppConfig::default(), |config| {
                normalize_config(config);
                Ok(())
            })
            .unwrap();
        assert!(!first.telemetry.enabled && !again.telemetry.enabled);
        assert_eq!(first.telemetry.anonymous_id, again.telemetry.anonymous_id);
        assert_eq!(before, fs::read(&path).unwrap());
    }

    #[test]
    fn 更新默认项不会改变已取得的业务快照且清除不会回填旧证书() {
        let dir = tempfile::tempdir().unwrap();
        let state = AppConfigState::new(dir.path().join("config.toml"), AppConfig::default());
        state
            .update_protect_defaults(
                serde_json::from_str(r#"{"certificate_id":"first","environment_policy":"strict"}"#)
                    .unwrap(),
            )
            .unwrap();
        let snapshot = state.read().unwrap().protect_defaults;
        state
            .update_protect_defaults(
                serde_json::from_str(
                    r#"{"certificate_id":null,"environment_policy":"compatible"}"#,
                )
                .unwrap(),
            )
            .unwrap();
        assert_eq!(snapshot.certificate_id.as_deref(), Some("first"));
        assert_eq!(snapshot.environment_policy, "strict");
        assert_eq!(state.read().unwrap().protect_defaults.certificate_id, None);
        let patch: AppConfigUpdate =
            serde_json::from_str(r#"{"locale":"en","theme_mode":"invalid"}"#).unwrap();
        assert!(state.update_preferences(patch).is_err());
        assert_eq!(state.read().unwrap().locale, "zh");
    }

    #[test]
    fn application_旧配置缺少应用分享分区时保持兼容且新包名默认开启() {
        let config: AppConfig = toml::from_str(
            r#"
locale = "zh"
theme_mode = "system"
"#,
        )
        .expect("旧配置应可解析");

        assert!(config.application_sharing.preferences.is_empty());
        assert!(config.application_sharing.enabled_for("com.example.new"));
    }

    #[test]
    fn application_全量设置保存不会覆盖已有按包名偏好() {
        let dir = tempfile::tempdir().expect("应创建临时目录");
        let state = AppConfigState::new(dir.path().join("config.toml"), AppConfig::default());
        state
            .mutate(|config| {
                config
                    .application_sharing
                    .preferences
                    .insert("com.example.app".to_string(), false);
            })
            .expect("偏好应保存");
        state
            .mutate(|config| {
                config.locale = "en".to_string();
                config.theme_mode = "dark".to_string();
            })
            .expect("设置应保存");

        let persisted: AppConfig = toml::from_str(
            &fs::read_to_string(dir.path().join("config.toml")).expect("应读取配置"),
        )
        .expect("配置应有效");
        assert!(!persisted.application_sharing.enabled_for("com.example.app"));
    }

    #[test]
    fn application_配置损坏时解析失败而不是默认同意() {
        assert!(toml::from_str::<AppConfig>("[application_sharing\npreferences = {}").is_err());
    }

    #[test]
    fn application_并发配置变更写盘后不会丢失包名偏好() {
        let dir = tempfile::tempdir().expect("应创建临时目录");
        let state = Arc::new(AppConfigState::new(
            dir.path().join("config.toml"),
            AppConfig::default(),
        ));
        let barrier = Arc::new(Barrier::new(9));
        let handles = (0..8)
            .map(|index| {
                let state = Arc::clone(&state);
                let barrier = Arc::clone(&barrier);
                std::thread::spawn(move || {
                    barrier.wait();
                    state.mutate(|config| {
                        config
                            .application_sharing
                            .preferences
                            .insert(format!("com.example.app{index}"), false);
                    })
                })
            })
            .collect::<Vec<_>>();
        barrier.wait();
        for handle in handles {
            handle
                .join()
                .expect("配置线程不应崩溃")
                .expect("配置应保存");
        }
        let persisted: AppConfig = toml::from_str(
            &fs::read_to_string(dir.path().join("config.toml")).expect("应读取配置"),
        )
        .expect("配置应有效");
        assert_eq!(persisted.application_sharing.preferences.len(), 8);
        assert!(persisted
            .application_sharing
            .preferences
            .values()
            .all(|enabled| !enabled));
    }

    #[test]
    fn 旧配置缺少加固默认项时使用兼容默认值() {
        let config: AppConfig = toml::from_str(
            r#"
locale = "zh"
theme_mode = "system"
"#,
        )
        .expect("旧配置应可解析");

        assert_eq!(config.protect_defaults.runtime_mode, "standard");
        assert_eq!(config.protect_defaults.environment_policy, "compatible");
        assert_eq!(config.protect_defaults.sign_after_protect, None);
        assert_eq!(config.protect_defaults.certificate_id, None);
        assert_eq!(config.protect_defaults.output_directory_mode, "source");
    }

    #[test]
    fn 非法加固默认项会被规范化() {
        let normalized = normalize_protect_defaults(&ProtectDefaults {
            runtime_mode: "unknown".to_string(),
            environment_policy: "unknown".to_string(),
            sign_after_protect: Some(true),
            certificate_id: Some("  cert-1  ".to_string()),
            output_directory_mode: "unknown".to_string(),
            fixed_output_directory: "  /tmp/output  ".to_string(),
        });

        assert_eq!(normalized.runtime_mode, "standard");
        assert_eq!(normalized.environment_policy, "compatible");
        assert_eq!(normalized.sign_after_protect, Some(true));
        assert_eq!(normalized.certificate_id.as_deref(), Some("cert-1"));
        assert_eq!(normalized.output_directory_mode, "source");
        assert_eq!(normalized.fixed_output_directory, "/tmp/output");
    }
}

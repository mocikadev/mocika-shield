use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use tauri::Manager;

const CONFIG_FILE: &str = "config.toml";

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
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub(crate) struct DailyTelemetry {
    /// 与 `daily` 的复合键冗余保存，避免迁移或上传时从键名反向解析。
    pub usage_date: String,
    /// 仅来自桌面端编译版本，不接受前端或 APK 输入。
    pub app_version: String,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct ProtectDefaults {
    pub runtime_mode: String,
    pub environment_policy: String,
    pub sign_after_protect: Option<bool>,
    pub certificate_id: Option<String>,
    pub output_directory_mode: String,
    pub fixed_output_directory: String,
}

impl Default for ProtectDefaults {
    fn default() -> Self {
        Self {
            runtime_mode: "standard".to_string(),
            environment_policy: "compatible".to_string(),
            sign_after_protect: None,
            certificate_id: None,
            output_directory_mode: "source".to_string(),
            fixed_output_directory: String::new(),
        }
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
    application_sharing_blocked: Mutex<std::collections::BTreeSet<String>>,
}

impl AppConfigState {
    pub(crate) fn new(path: PathBuf, config: AppConfig) -> Self {
        Self {
            path,
            config: Mutex::new(config),
            application_sharing_blocked: Mutex::new(std::collections::BTreeSet::new()),
        }
    }

    pub(crate) fn read(&self) -> Result<AppConfig, String> {
        self.config
            .lock()
            .map(|cfg| cfg.clone())
            .map_err(|_| "读取配置失败：配置状态锁已损坏".to_string())
    }

    pub(crate) fn mutate<F>(&self, mutator: F) -> Result<(), String>
    where
        F: FnOnce(&mut AppConfig),
    {
        let mut cfg = self
            .config
            .lock()
            .map_err(|_| "写入配置失败：配置状态锁已损坏".to_string())?;
        let original = cfg.clone();
        mutator(&mut cfg);
        cfg.locale = normalize_locale(&cfg.locale);
        cfg.theme_mode = normalize_theme_mode(&cfg.theme_mode);
        cfg.protect_defaults = normalize_protect_defaults(&cfg.protect_defaults);
        cfg.dismissed_version = cfg
            .dismissed_version
            .take()
            .filter(|value| !value.trim().is_empty());
        // 写盘属于同一临界区，避免并发更新用旧快照覆盖较新的包名偏好。
        if let Err(error) = save_app_config_file(&self.path, &cfg) {
            *cfg = original;
            return Err(error);
        }
        Ok(())
    }

    pub(crate) fn application_sharing_enabled(&self, package_name: &str) -> Result<bool, String> {
        let config = self.read()?;
        let blocked = self
            .application_sharing_blocked
            .lock()
            .map_err(|_| "读取应用分享状态失败：状态锁已损坏".to_string())?;
        Ok(!blocked.contains(package_name) && config.application_sharing.enabled_for(package_name))
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

pub(crate) fn normalize_protect_defaults(value: &ProtectDefaults) -> ProtectDefaults {
    ProtectDefaults {
        runtime_mode: match value.runtime_mode.as_str() {
            "android_api19" => "android_api19",
            _ => "standard",
        }
        .to_string(),
        environment_policy: match value.environment_policy.as_str() {
            "strict" => "strict",
            _ => "compatible",
        }
        .to_string(),
        sign_after_protect: value.sign_after_protect,
        certificate_id: value
            .certificate_id
            .as_deref()
            .map(str::trim)
            .filter(|id| !id.is_empty())
            .map(str::to_string),
        output_directory_mode: match value.output_directory_mode.as_str() {
            "fixed" => "fixed",
            _ => "source",
        }
        .to_string(),
        fixed_output_directory: value.fixed_output_directory.trim().to_string(),
    }
}

pub(crate) fn normalize_keystore_type(value: Option<&str>) -> Option<String> {
    match value.map(|s| s.trim()).filter(|s| !s.is_empty()) {
        Some(raw) if raw.eq_ignore_ascii_case("pkcs12") || raw.eq_ignore_ascii_case("p12") => {
            Some("PKCS12".to_string())
        }
        Some(_) => Some("JKS".to_string()),
        None => Some("JKS".to_string()),
    }
}

fn config_file_path(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_config_dir()
        .map_err(|e| format!("无法定位配置目录: {e}"))?;
    fs::create_dir_all(&dir).map_err(|e| format!("创建配置目录失败: {e}"))?;
    Ok(dir.join(CONFIG_FILE))
}

pub(crate) fn load_app_config(app: &tauri::AppHandle) -> Result<LoadedAppConfig, String> {
    let path = config_file_path(app)?;
    if path.exists() {
        let raw = fs::read_to_string(&path).map_err(|e| format!("读取配置文件失败: {e}"))?;
        let mut config: AppConfig =
            toml::from_str(&raw).map_err(|e| format!("解析配置文件失败: {e}"))?;
        config.locale = normalize_locale(&config.locale);
        config.theme_mode = normalize_theme_mode(&config.theme_mode);
        config.protect_defaults = normalize_protect_defaults(&config.protect_defaults);
        config.dismissed_version = config
            .dismissed_version
            .take()
            .filter(|value| !value.trim().is_empty());
        return Ok(LoadedAppConfig { path, config });
    }

    Ok(LoadedAppConfig {
        path,
        config: AppConfig::default(),
    })
}

pub(crate) fn save_app_config_file(path: &Path, config: &AppConfig) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("创建配置目录失败: {e}"))?;
    }
    let body = toml::to_string_pretty(config).map_err(|e| format!("序列化配置失败: {e}"))?;
    fs::write(path, body).map_err(|e| format!("写入配置文件失败: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Barrier};

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

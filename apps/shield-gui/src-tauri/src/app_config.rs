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
    pub app_start_count: u32,
    pub protect_start_count: u32,
    pub protect_success_count: u32,
    pub protect_failed_count: u32,
    pub sign_success_count: u32,
    pub sign_failed_count: u32,
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
}

impl AppConfigState {
    pub(crate) fn new(path: PathBuf, config: AppConfig) -> Self {
        Self {
            path,
            config: Mutex::new(config),
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
        let snapshot = {
            let mut cfg = self
                .config
                .lock()
                .map_err(|_| "写入配置失败：配置状态锁已损坏".to_string())?;
            mutator(&mut cfg);
            cfg.locale = normalize_locale(&cfg.locale);
            cfg.theme_mode = normalize_theme_mode(&cfg.theme_mode);
            cfg.protect_defaults = normalize_protect_defaults(&cfg.protect_defaults);
            cfg.dismissed_version = cfg
                .dismissed_version
                .take()
                .filter(|value| !value.trim().is_empty());
            cfg.clone()
        };
        save_app_config_file(&self.path, &snapshot)
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

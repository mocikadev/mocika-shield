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
pub(crate) struct AppConfig {
    pub locale: String,
    pub theme_mode: String,
    pub dismissed_version: Option<String>,
    pub update_cache: UpdateCache,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            locale: "zh".to_string(),
            theme_mode: "system".to_string(),
            dismissed_version: None,
            update_cache: UpdateCache::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct AppConfigPayload {
    pub locale: String,
    pub theme_mode: String,
}

impl From<&AppConfig> for AppConfigPayload {
    fn from(config: &AppConfig) -> Self {
        Self {
            locale: normalize_locale(&config.locale),
            theme_mode: normalize_theme_mode(&config.theme_mode),
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

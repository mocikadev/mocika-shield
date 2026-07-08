use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use tauri::Manager;

const CONFIG_FILE: &str = "config.toml";
const LEGACY_STORE_FILE: &str = "tool_config.json";
const STORE_KEY_KEYSTORE: &str = "keystore_path";
const STORE_KEY_KEY_ALIAS: &str = "key_alias";
const STORE_KEY_AUTO_SIGN: &str = "auto_sign_enabled";
const STORE_KEY_KS_TYPE: &str = "ks_type";
const STORE_KEY_SIGN_V1: &str = "sign_v1";
const STORE_KEY_SIGN_V2: &str = "sign_v2";
const STORE_KEY_SIGN_V3: &str = "sign_v3";
const STORE_KEY_SIGN_V4: &str = "sign_v4";
const STORE_KEY_KS_PASS: &str = "ks_password";
const STORE_KEY_KEY_PASS: &str = "key_password";
const STORE_KEY_LOCALE: &str = "locale";
const STORE_KEY_UPDATE_LAST_CHECK: &str = "update_last_check";
const STORE_KEY_UPDATE_LATEST_TAG: &str = "update_latest_tag";
const STORE_KEY_UPDATE_RELEASE_URL: &str = "update_release_url";
const STORE_KEY_UPDATE_DISMISSED: &str = "dismissed_version";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct SignConfig {
    pub keystore_path: Option<String>,
    pub key_alias: Option<String>,
    pub auto_sign_enabled: bool,
    pub ks_type: Option<String>,
    pub sign_v1: bool,
    pub sign_v2: bool,
    pub sign_v3: bool,
    pub sign_v4: bool,
}

impl Default for SignConfig {
    fn default() -> Self {
        Self {
            keystore_path: None,
            key_alias: None,
            auto_sign_enabled: false,
            ks_type: Some("JKS".to_string()),
            sign_v1: true,
            sign_v2: true,
            sign_v3: true,
            sign_v4: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct StoredSigningConfig {
    pub keystore_path: Option<String>,
    pub key_alias: Option<String>,
    pub auto_sign_enabled: bool,
    pub ks_type: Option<String>,
    pub sign_v1: bool,
    pub sign_v2: bool,
    pub sign_v3: bool,
    pub sign_v4: bool,
    pub keystore_password: String,
    pub key_password: String,
}

impl Default for StoredSigningConfig {
    fn default() -> Self {
        Self {
            keystore_path: None,
            key_alias: None,
            auto_sign_enabled: false,
            ks_type: Some("JKS".to_string()),
            sign_v1: true,
            sign_v2: true,
            sign_v3: true,
            sign_v4: false,
            keystore_password: String::new(),
            key_password: String::new(),
        }
    }
}

impl StoredSigningConfig {
    pub(crate) fn to_sign_config(&self) -> SignConfig {
        SignConfig {
            keystore_path: self.keystore_path.clone().filter(|s| !s.is_empty()),
            key_alias: self.key_alias.clone().filter(|s| !s.is_empty()),
            auto_sign_enabled: self.auto_sign_enabled,
            ks_type: normalize_keystore_type(self.ks_type.as_deref()),
            sign_v1: self.sign_v1,
            sign_v2: self.sign_v2,
            sign_v3: self.sign_v3,
            sign_v4: self.sign_v4,
        }
    }

    pub(crate) fn apply_sign_config(&mut self, config: SignConfig) {
        self.keystore_path = config.keystore_path.filter(|s| !s.is_empty());
        self.key_alias = config.key_alias.filter(|s| !s.is_empty());
        self.auto_sign_enabled = config.auto_sign_enabled;
        self.ks_type = normalize_keystore_type(config.ks_type.as_deref());
        self.sign_v1 = config.sign_v1;
        self.sign_v2 = config.sign_v2;
        self.sign_v3 = config.sign_v3;
        self.sign_v4 = config.sign_v4;
    }
}

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
    pub signing: StoredSigningConfig,
    pub dismissed_version: Option<String>,
    pub update_cache: UpdateCache,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            locale: "zh".to_string(),
            theme_mode: "system".to_string(),
            signing: StoredSigningConfig::default(),
            dismissed_version: None,
            update_cache: UpdateCache::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct AppConfigPayload {
    pub locale: String,
    pub theme_mode: String,
    pub sign_config: SignConfig,
    pub keystore_password: String,
    pub key_password: String,
}

impl From<&AppConfig> for AppConfigPayload {
    fn from(config: &AppConfig) -> Self {
        Self {
            locale: normalize_locale(&config.locale),
            theme_mode: normalize_theme_mode(&config.theme_mode),
            sign_config: config.signing.to_sign_config(),
            keystore_password: config.signing.keystore_password.clone(),
            key_password: config.signing.key_password.clone(),
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
            cfg.signing.ks_type = normalize_keystore_type(cfg.signing.ks_type.as_deref());
            cfg.signing.keystore_path = cfg.signing.keystore_path.take().filter(|s| !s.is_empty());
            cfg.signing.key_alias = cfg.signing.key_alias.take().filter(|s| !s.is_empty());
            cfg.clone()
        };
        save_app_config_file(&self.path, &snapshot)
    }
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

fn legacy_store_paths(app: &tauri::AppHandle) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Ok(dir) = app.path().app_config_dir() {
        paths.push(dir.join(LEGACY_STORE_FILE));
    }
    if let Ok(dir) = app.path().app_data_dir() {
        let candidate = dir.join(LEGACY_STORE_FILE);
        if !paths.iter().any(|item| item == &candidate) {
            paths.push(candidate);
        }
    }
    paths
}

fn migrate_legacy_store(path: &Path) -> Option<AppConfig> {
    let raw = fs::read_to_string(path).ok()?;
    let json: serde_json::Value = serde_json::from_str(&raw).ok()?;
    let mut config = AppConfig::default();
    config.locale = normalize_locale(
        json.get(STORE_KEY_LOCALE)
            .and_then(|v| v.as_str())
            .unwrap_or("zh"),
    );
    config.signing.keystore_path = json
        .get(STORE_KEY_KEYSTORE)
        .and_then(|v| v.as_str())
        .map(str::to_string)
        .filter(|s| !s.is_empty());
    config.signing.key_alias = json
        .get(STORE_KEY_KEY_ALIAS)
        .and_then(|v| v.as_str())
        .map(str::to_string)
        .filter(|s| !s.is_empty());
    config.signing.auto_sign_enabled = json
        .get(STORE_KEY_AUTO_SIGN)
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    config.signing.ks_type =
        normalize_keystore_type(json.get(STORE_KEY_KS_TYPE).and_then(|v| v.as_str()));
    config.signing.sign_v1 = json
        .get(STORE_KEY_SIGN_V1)
        .and_then(|v| v.as_bool())
        .unwrap_or(true);
    config.signing.sign_v2 = json
        .get(STORE_KEY_SIGN_V2)
        .and_then(|v| v.as_bool())
        .unwrap_or(true);
    config.signing.sign_v3 = json
        .get(STORE_KEY_SIGN_V3)
        .and_then(|v| v.as_bool())
        .unwrap_or(true);
    config.signing.sign_v4 = json
        .get(STORE_KEY_SIGN_V4)
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    config.signing.keystore_password = json
        .get(STORE_KEY_KS_PASS)
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();
    config.signing.key_password = json
        .get(STORE_KEY_KEY_PASS)
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();
    config.dismissed_version = json
        .get(STORE_KEY_UPDATE_DISMISSED)
        .and_then(|v| v.as_str())
        .map(str::to_string)
        .filter(|s| !s.is_empty());
    config.update_cache.last_check = json
        .get(STORE_KEY_UPDATE_LAST_CHECK)
        .and_then(|v| v.as_i64());
    config.update_cache.latest_tag = json
        .get(STORE_KEY_UPDATE_LATEST_TAG)
        .and_then(|v| v.as_str())
        .map(str::to_string)
        .filter(|s| !s.is_empty());
    config.update_cache.release_url = json
        .get(STORE_KEY_UPDATE_RELEASE_URL)
        .and_then(|v| v.as_str())
        .map(str::to_string)
        .filter(|s| !s.is_empty());
    Some(config)
}

pub(crate) fn load_app_config(
    app: &tauri::AppHandle,
) -> Result<(PathBuf, AppConfig, Option<PathBuf>), String> {
    let path = config_file_path(app)?;
    if path.exists() {
        let raw = fs::read_to_string(&path).map_err(|e| format!("读取配置文件失败: {e}"))?;
        let mut config: AppConfig =
            toml::from_str(&raw).map_err(|e| format!("解析配置文件失败: {e}"))?;
        config.locale = normalize_locale(&config.locale);
        config.theme_mode = normalize_theme_mode(&config.theme_mode);
        config.signing.ks_type = normalize_keystore_type(config.signing.ks_type.as_deref());
        config.signing.keystore_path = config
            .signing
            .keystore_path
            .take()
            .filter(|s| !s.is_empty());
        config.signing.key_alias = config.signing.key_alias.take().filter(|s| !s.is_empty());
        return Ok((path, config, None));
    }

    for legacy in legacy_store_paths(app) {
        if legacy.exists() {
            if let Some(config) = migrate_legacy_store(&legacy) {
                return Ok((path, config, Some(legacy)));
            }
        }
    }

    Ok((path, AppConfig::default(), None))
}

pub(crate) fn save_app_config_file(path: &Path, config: &AppConfig) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("创建配置目录失败: {e}"))?;
    }
    let body = toml::to_string_pretty(config).map_err(|e| format!("序列化配置失败: {e}"))?;
    fs::write(path, body).map_err(|e| format!("写入配置文件失败: {e}"))
}

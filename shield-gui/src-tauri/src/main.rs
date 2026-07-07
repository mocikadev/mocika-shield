#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use serde::{Deserialize, Serialize};
use shield_cli::{
    protect_apk as shield_protect_apk, sign_apk as shield_sign_apk, KeystoreType, ProgressEvent,
    ProtectOptions, ShieldError, SignOptions, SigningVersions,
};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use tauri::{Emitter, Manager};
use tauri_plugin_store::StoreExt;

fn no_window_command(prog: &str) -> std::process::Command {
    #[allow(unused_mut)]
    let mut cmd = std::process::Command::new(prog);
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

struct CancelHandle(Arc<AtomicBool>);

const STORE_FILE: &str = "tool_config.json";
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

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct SignConfig {
    keystore_path: Option<String>,
    key_alias: Option<String>,
    auto_sign_enabled: bool,
    ks_type: Option<String>,
    sign_v1: bool,
    sign_v2: bool,
    sign_v3: bool,
    sign_v4: bool,
}

fn load_sign_config(app: &tauri::AppHandle) -> Result<SignConfig, String> {
    let store = app.store(STORE_FILE).map_err(|e| e.to_string())?;
    Ok(SignConfig {
        keystore_path: store
            .get(STORE_KEY_KEYSTORE)
            .and_then(|v| v.as_str().map(|s| s.to_string()))
            .filter(|s| !s.is_empty()),
        key_alias: store
            .get(STORE_KEY_KEY_ALIAS)
            .and_then(|v| v.as_str().map(|s| s.to_string()))
            .filter(|s| !s.is_empty()),
        auto_sign_enabled: store
            .get(STORE_KEY_AUTO_SIGN)
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
        ks_type: store
            .get(STORE_KEY_KS_TYPE)
            .and_then(|v| v.as_str().map(|s| s.to_string()))
            .filter(|s| !s.is_empty()),
        sign_v1: store
            .get(STORE_KEY_SIGN_V1)
            .and_then(|v| v.as_bool())
            .unwrap_or(true),
        sign_v2: store
            .get(STORE_KEY_SIGN_V2)
            .and_then(|v| v.as_bool())
            .unwrap_or(true),
        sign_v3: store
            .get(STORE_KEY_SIGN_V3)
            .and_then(|v| v.as_bool())
            .unwrap_or(true),
        sign_v4: store
            .get(STORE_KEY_SIGN_V4)
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
    })
}

fn store_sign_config(app: &tauri::AppHandle, config: SignConfig) -> Result<(), String> {
    let store = app.store(STORE_FILE).map_err(|e| e.to_string())?;
    store.set(
        STORE_KEY_KEYSTORE,
        serde_json::Value::String(config.keystore_path.unwrap_or_default()),
    );
    store.set(
        STORE_KEY_KEY_ALIAS,
        serde_json::Value::String(config.key_alias.unwrap_or_default()),
    );
    store.set(
        STORE_KEY_AUTO_SIGN,
        serde_json::Value::Bool(config.auto_sign_enabled),
    );
    store.set(
        STORE_KEY_KS_TYPE,
        serde_json::Value::String(config.ks_type.unwrap_or_else(|| "JKS".to_string())),
    );
    store.set(STORE_KEY_SIGN_V1, serde_json::Value::Bool(config.sign_v1));
    store.set(STORE_KEY_SIGN_V2, serde_json::Value::Bool(config.sign_v2));
    store.set(STORE_KEY_SIGN_V3, serde_json::Value::Bool(config.sign_v3));
    store.set(STORE_KEY_SIGN_V4, serde_json::Value::Bool(config.sign_v4));
    store.save().map_err(|e| e.to_string())
}

fn load_sign_passwords(app: &tauri::AppHandle) -> Result<(String, String), String> {
    let store = app.store(STORE_FILE).map_err(|e| e.to_string())?;
    let ks_pass = store
        .get(STORE_KEY_KS_PASS)
        .and_then(|v| v.as_str().map(|s| s.to_string()))
        .unwrap_or_default();
    let key_pass = store
        .get(STORE_KEY_KEY_PASS)
        .and_then(|v| v.as_str().map(|s| s.to_string()))
        .unwrap_or_default();
    Ok((ks_pass, key_pass))
}

fn store_sign_passwords(
    app: &tauri::AppHandle,
    keystore_password: String,
    key_password: String,
) -> Result<(), String> {
    let store = app.store(STORE_FILE).map_err(|e| e.to_string())?;
    store.set(
        STORE_KEY_KS_PASS,
        serde_json::Value::String(keystore_password),
    );
    store.set(STORE_KEY_KEY_PASS, serde_json::Value::String(key_password));
    store.save().map_err(|e| e.to_string())
}

fn execute_sign_apk(
    app: &tauri::AppHandle,
    apk_path: String,
    output_path: Option<String>,
    apksigner_path: Option<String>,
    keystore_path: String,
    key_alias: String,
    ks_type: Option<String>,
    sign_v1: bool,
    sign_v2: bool,
    sign_v3: bool,
    sign_v4: bool,
) -> Result<(), String> {
    let resolved_apksigner = apksigner_path
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .or_else(|| find_apksigner_path(app));
    let (ks_pass, key_pass) = load_sign_passwords(app)?;
    let effective_key_pass = if key_pass.is_empty() {
        ks_pass.clone()
    } else {
        key_pass
    };
    let opts = SignOptions {
        apk_path: PathBuf::from(apk_path),
        output_path: output_path.filter(|s| !s.is_empty()).map(PathBuf::from),
        keystore_path: PathBuf::from(keystore_path),
        key_alias,
        keystore_password: ks_pass,
        key_password: effective_key_pass,
        apksigner_path: resolved_apksigner,
        keystore_type: KeystoreType::from_str(ks_type.as_deref().unwrap_or("JKS")),
        signing_versions: SigningVersions {
            v1: sign_v1,
            v2: sign_v2,
            v3: sign_v3,
            v4: sign_v4,
        },
    };
    shield_sign_apk(&opts).map_err(|e| e.to_string())
}

fn query_keystore_aliases(
    keystore_path: String,
    ks_pass: String,
    ks_type: Option<String>,
) -> Result<Vec<String>, String> {
    let ks_type_str = ks_type.as_deref().unwrap_or("JKS");
    let output = no_window_command("keytool")
        .args([
            "-list",
            "-keystore",
            &keystore_path,
            "-storetype",
            ks_type_str,
            "-storepass",
            &ks_pass,
        ])
        .output()
        .map_err(|e| format!("启动 keytool 失败，请确认 Java 已安装: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("keytool 执行失败: {}", stderr.trim()));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let aliases = parse_keytool_aliases(&stdout);

    if aliases.is_empty() {
        Err("未在 keystore 中找到任何 alias".to_string())
    } else {
        Ok(aliases)
    }
}

fn parse_keytool_aliases(output: &str) -> Vec<String> {
    let mut aliases = Vec::new();
    for line in output.lines() {
        let trimmed = line.trim();
        if trimmed.contains("PrivateKeyEntry")
            || trimmed.contains("SecretKeyEntry")
            || trimmed.contains("trustedCertEntry")
        {
            if let Some(alias) = trimmed.split(',').next() {
                let alias = alias.trim().to_string();
                if !alias.is_empty() {
                    aliases.push(alias);
                }
            }
        }
    }
    aliases
}

#[derive(Debug, Clone, Serialize)]
struct ProtectProgressPayload {
    step: String,
    message: String,
}

async fn execute_protect_apk(
    window: tauri::Window,
    input: String,
    output: String,
    apktool_path: Option<String>,
    resources_path: Option<String>,
    cancel_handle: tauri::State<'_, CancelHandle>,
) -> Result<(), String> {
    cancel_handle.inner().0.store(false, Ordering::SeqCst);

    let app = window.app_handle().clone();

    let resolved_apktool = apktool_path
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .or_else(|| find_apktool_path(&app));

    let resolved_resources = resources_path
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .or_else(|| find_resources_path(&app));

    let cancel = Arc::clone(&cancel_handle.inner().0);
    let emit_error = Arc::new(Mutex::new(None::<String>));
    let emit_error_for_task = Arc::clone(&emit_error);
    let progress_window = window.clone();

    let task_result = tokio::task::spawn_blocking(move || {
        let opts = ProtectOptions {
            input: strip_unc_prefix(PathBuf::from(input)),
            output: strip_unc_prefix(PathBuf::from(output)),
            apktool_path: resolved_apktool,
            resources_path: resolved_resources,
        };
        let cancel_for_progress = Arc::clone(&cancel);
        let cancel_for_protect = Arc::clone(&cancel);

        shield_protect_apk(
            &opts,
            move |event: ProgressEvent| {
                let payload = ProtectProgressPayload {
                    step: format!("{:?}", event.step),
                    message: event.message,
                };
                if let Err(err) = progress_window.emit("protect-progress", &payload) {
                    if let Ok(mut slot) = emit_error_for_task.lock() {
                        if slot.is_none() {
                            *slot = Some(format!("发送进度事件失败: {err}"));
                        }
                    }
                    cancel_for_progress.store(true, Ordering::SeqCst);
                }
            },
            cancel_for_protect,
        )
    })
    .await
    .map_err(|err| {
        let msg = format!("后台任务执行失败: {err}");
        emit_protect_error(&window, &msg);
        msg
    })?;

    let progress_emit_error = emit_error
        .lock()
        .map_err(|_| {
            let msg = "发送进度事件失败：状态锁已损坏".to_string();
            emit_protect_error(&window, &msg);
            msg
        })?
        .clone();

    if let Some(msg) = progress_emit_error {
        emit_protect_error(&window, &msg);
        return Err(msg);
    }

    match task_result {
        Ok(()) => {
            window.emit("protect-done", ()).map_err(|err| {
                let msg = format!("发送完成事件失败: {err}");
                emit_protect_error(&window, &msg);
                msg
            })?;
            Ok(())
        }
        Err(ShieldError::Cancelled) => Err("已取消".to_string()),
        Err(err) => {
            let msg = err.to_string();
            emit_protect_error(&window, &msg);
            Err(msg)
        }
    }
}

fn emit_protect_error(window: &tauri::Window, message: &str) {
    let _ = window.emit("protect-error", message.to_string());
}

#[derive(Debug, Serialize)]
struct ApkCheckResult {
    already_protected: bool,
    is_signed: bool,
    error: Option<String>,
}

#[derive(Debug, Serialize)]
struct CertCompareResult {
    matches: bool,
    apk_fingerprint: Option<String>,
    ks_fingerprint: Option<String>,
    error: Option<String>,
}

fn do_compare_cert_fingerprints(
    apk_path: String,
    keystore_path: String,
    ks_pass: String,
    ks_type: Option<String>,
    key_alias: String,
    apksigner_path: Option<PathBuf>,
) -> CertCompareResult {
    let apk_fp = extract_apk_fingerprint(&apk_path, apksigner_path.as_deref());
    let ks_fp =
        extract_keystore_fingerprint(&keystore_path, &ks_pass, ks_type.as_deref(), &key_alias);

    match (apk_fp, ks_fp) {
        (Ok(apk), Ok(ks)) => {
            let matches = normalize_fingerprint(&apk) == normalize_fingerprint(&ks);
            CertCompareResult {
                matches,
                apk_fingerprint: Some(apk),
                ks_fingerprint: Some(ks),
                error: None,
            }
        }
        (Err(e), _) => CertCompareResult {
            matches: false,
            apk_fingerprint: None,
            ks_fingerprint: None,
            error: Some(format!("读取 APK 签名失败: {e}")),
        },
        (_, Err(e)) => CertCompareResult {
            matches: false,
            apk_fingerprint: None,
            ks_fingerprint: None,
            error: Some(format!("读取 keystore 失败: {e}")),
        },
    }
}

fn extract_apk_fingerprint(
    apk_path: &str,
    apksigner_path: Option<&Path>,
) -> Result<String, String> {
    if let Ok(output) = no_window_command("keytool")
        .args(["-printcert", "-jarfile", apk_path])
        .output()
    {
        if output.status.success() {
            let text = String::from_utf8_lossy(&output.stdout);
            if let Some(fp) = parse_sha256_fingerprint(&text) {
                return Ok(fp);
            }
        }
    }

    let signer =
        apksigner_path.ok_or_else(|| "V1 证书提取失败，且未找到 apksigner.jar".to_string())?;
    let result = no_window_command("java")
        .args([
            "-jar",
            signer.to_str().unwrap_or(""),
            "verify",
            "--print-certs",
            apk_path,
        ])
        .output()
        .map_err(|e| format!("执行 apksigner verify 失败: {e}"))?;

    if result.status.success() {
        let stdout = String::from_utf8_lossy(&result.stdout);
        if let Some(fp) = parse_sha256_from_apksigner(&stdout) {
            return Ok(fp);
        }
    }

    Err("无法提取 APK 证书指纹（V1 和 V2/V3 均失败）".to_string())
}

fn extract_keystore_fingerprint(
    keystore_path: &str,
    ks_pass: &str,
    ks_type: Option<&str>,
    key_alias: &str,
) -> Result<String, String> {
    let mut args = vec![
        "-list",
        "-v",
        "-keystore",
        keystore_path,
        "-alias",
        key_alias,
        "-storepass",
        ks_pass,
    ];
    if let Some(t) = ks_type {
        args.push("-storetype");
        args.push(t);
    }

    let output = no_window_command("keytool")
        .args(&args)
        .output()
        .map_err(|e| format!("启动 keytool 失败: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("keytool 执行失败: {}", stderr.trim()));
    }

    let text = String::from_utf8_lossy(&output.stdout);
    parse_sha256_fingerprint(&text).ok_or_else(|| "未能从 keystore 中解析 SHA256 指纹".to_string())
}

fn parse_sha256_fingerprint(text: &str) -> Option<String> {
    for line in text.lines() {
        let trimmed = line.trim();
        if let Some(pos) = trimmed.to_uppercase().find("SHA256") {
            let after = &trimmed[pos..];
            if let Some(colon_pos) = after.find(':') {
                let fp = after[colon_pos + 1..].trim();
                if fp.contains(':') && fp.len() > 30 {
                    return Some(fp.to_string());
                }
            }
        }
    }
    None
}

fn parse_sha256_from_apksigner(output: &str) -> Option<String> {
    for line in output.lines() {
        let upper = line.trim().to_uppercase();
        if upper.contains("SHA-256") && upper.contains("DIGEST") {
            if let Some(pos) = line.rfind(':') {
                let hex = line[pos + 1..].trim().to_uppercase();
                if hex.len() == 64 && hex.chars().all(|c| c.is_ascii_hexdigit()) {
                    return Some(hex);
                }
            }
        }
    }
    None
}

fn normalize_fingerprint(fp: &str) -> String {
    fp.to_uppercase().replace(':', "").replace(' ', "")
}

fn do_check_apk(path: String, apksigner_path: Option<PathBuf>) -> ApkCheckResult {
    let apk_path = PathBuf::from(&path);

    let file = match fs::File::open(&apk_path) {
        Ok(f) => f,
        Err(e) => {
            return ApkCheckResult {
                already_protected: false,
                is_signed: false,
                error: Some(format!("无法打开 APK 文件: {e}")),
            }
        }
    };

    let mut archive = match zip::ZipArchive::new(file) {
        Ok(a) => a,
        Err(e) => {
            return ApkCheckResult {
                already_protected: false,
                is_signed: false,
                error: Some(format!("无法解析 APK（ZIP 格式错误）: {e}")),
            }
        }
    };

    let mut already_protected = false;
    let mut classes_dex_error: Option<String> = None;

    const MSHD_MAGIC: &[u8] = b"MSHD";
    const TAIL_READ_SIZE: u64 = 4096;

    for i in 0..archive.len() {
        let mut entry = match archive.by_index(i) {
            Ok(e) => e,
            Err(_) => continue,
        };
        let name = entry.name().to_owned();

        if name == "classes.dex" && !already_protected {
            use std::io::Read;
            let entry_size = entry.size();
            if entry_size >= 8 {
                let skip = entry_size.saturating_sub(TAIL_READ_SIZE);
                let read_len = (entry_size - skip) as usize;
                let mut tail = vec![0u8; read_len];
                let skip_result = if skip > 0 {
                    std::io::copy(&mut entry.by_ref().take(skip), &mut std::io::sink())
                        .map(|_| ())
                        .map_err(|e| e.to_string())
                } else {
                    Ok(())
                };
                match skip_result
                    .and_then(|_| entry.read_exact(&mut tail).map_err(|e| e.to_string()))
                {
                    Ok(()) => {
                        already_protected = tail.windows(MSHD_MAGIC.len()).any(|w| w == MSHD_MAGIC);
                    }
                    Err(e) => {
                        classes_dex_error = Some(format!("classes.dex 读取失败: {e}"));
                    }
                }
            }
        }

        if already_protected {
            break;
        }
    }

    if let Some(err) = classes_dex_error {
        return ApkCheckResult {
            already_protected: false,
            is_signed: false,
            error: Some(err),
        };
    }

    // 使用 apksigner verify 检测签名（兼容 V1/V2/V3/V4）
    // 若找不到 apksigner，降级为检测 META-INF/*.RSA|DSA|EC（仅 V1）
    let is_signed = check_apk_signed(&apk_path, apksigner_path.as_deref());

    ApkCheckResult {
        already_protected,
        is_signed,
        error: None,
    }
}

/// 检测 APK 是否已签名。
/// 优先使用 apksigner verify（支持 V1/V2/V3/V4），退出码 0 表示已签名。
/// 若 apksigner 不可用，降级为扫描 META-INF/*.RSA|DSA|EC（仅 V1）。
fn check_apk_signed(apk_path: &PathBuf, apksigner_path: Option<&Path>) -> bool {
    if let Some(signer) = apksigner_path {
        // apksigner verify 成功退出码为 0，失败（未签名或签名无效）为非 0
        if let Ok(status) = no_window_command("java")
            .args([
                "-jar",
                signer.to_str().unwrap_or(""),
                "verify",
                apk_path.to_str().unwrap_or(""),
            ])
            .status()
        {
            return status.success();
        }
        // java 调用失败（如 java 未安装），降级
    }

    // 降级：扫描 META-INF/*.RSA|DSA|EC（仅检测 V1 签名）
    let Ok(file) = fs::File::open(apk_path) else {
        return false;
    };
    let Ok(mut archive) = zip::ZipArchive::new(file) else {
        return false;
    };
    for i in 0..archive.len() {
        let Ok(entry) = archive.by_index(i) else {
            continue;
        };
        let name = entry.name();
        if name.starts_with("META-INF/")
            && (name.ends_with(".RSA") || name.ends_with(".DSA") || name.ends_with(".EC"))
        {
            return true;
        }
    }
    false
}

fn strip_unc_prefix(path: PathBuf) -> PathBuf {
    dunce::simplified(&path).to_path_buf()
}

fn resource_dir(app: &tauri::AppHandle) -> Option<PathBuf> {
    app.path().resource_dir().ok().map(strip_unc_prefix)
}

fn appimage_resource_dir() -> Option<PathBuf> {
    let appdir = std::env::var("APPDIR").ok()?;
    Some(PathBuf::from(appdir).join("usr/lib/mocika-shield"))
}

fn find_apktool_path(app: &tauri::AppHandle) -> Option<PathBuf> {
    for base in resource_dir(app).into_iter().chain(appimage_resource_dir()) {
        let p = base.join("tools/apktool.jar");
        if p.exists() {
            return Some(p);
        }
    }
    let dev = project_root_path().join("tools/apktool_3.0.1.jar");
    if dev.exists() {
        return Some(dev);
    }
    None
}

fn find_resources_path(app: &tauri::AppHandle) -> Option<PathBuf> {
    for base in resource_dir(app).into_iter().chain(appimage_resource_dir()) {
        let p = base.join("resources/resources.zip");
        if p.exists() {
            return Some(p);
        }
    }
    let dev = project_root_path().join("shield-stub/build/outputs/resources/resources.zip");
    if dev.exists() {
        return Some(dev);
    }
    None
}

fn find_apksigner_path(app: &tauri::AppHandle) -> Option<PathBuf> {
    for base in resource_dir(app).into_iter().chain(appimage_resource_dir()) {
        let p = base.join("tools/apksigner.jar");
        if p.exists() {
            return Some(p);
        }
    }
    let dev = project_root_path().join("tools/apksigner.jar");
    if dev.exists() {
        return Some(dev);
    }
    None
}

fn project_root_path() -> PathBuf {
    let current = strip_unc_prefix(std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
    let mut path = current.clone();
    loop {
        if path.join("shield-stub").exists() && path.join("shield-cli").exists() {
            return path;
        }
        if !path.pop() {
            break;
        }
    }
    current
}

#[tauri::command]
async fn compare_cert_fingerprints(
    app: tauri::AppHandle,
    apk_path: String,
    keystore_path: String,
    ks_pass: String,
    ks_type: Option<String>,
    key_alias: String,
) -> CertCompareResult {
    let apksigner = find_apksigner_path(&app);
    tauri::async_runtime::spawn_blocking(move || {
        do_compare_cert_fingerprints(
            apk_path,
            keystore_path,
            ks_pass,
            ks_type,
            key_alias,
            apksigner,
        )
    })
    .await
    .unwrap_or_else(|e| CertCompareResult {
        matches: false,
        apk_fingerprint: None,
        ks_fingerprint: None,
        error: Some(format!("证书对比任务异常: {e}")),
    })
}

#[tauri::command]
async fn protect_apk(
    window: tauri::Window,
    input: String,
    output: String,
    apktool_path: Option<String>,
    resources_path: Option<String>,
    cancel_handle: tauri::State<'_, CancelHandle>,
) -> Result<(), String> {
    execute_protect_apk(
        window,
        input,
        output,
        apktool_path,
        resources_path,
        cancel_handle,
    )
    .await
}

#[tauri::command]
fn cancel_protect(cancel_handle: tauri::State<'_, CancelHandle>) {
    cancel_handle.inner().0.store(true, Ordering::SeqCst);
}

#[tauri::command]
fn show_in_folder(path: String) -> Result<(), String> {
    let dir = std::path::Path::new(&path)
        .parent()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or(path);
    #[cfg(target_os = "linux")]
    std::process::Command::new("xdg-open")
        .arg(&dir)
        .spawn()
        .map_err(|e| e.to_string())?;
    #[cfg(target_os = "macos")]
    std::process::Command::new("open")
        .arg(&dir)
        .spawn()
        .map_err(|e| e.to_string())?;
    #[cfg(target_os = "windows")]
    std::process::Command::new("explorer")
        .arg(&dir)
        .spawn()
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
fn check_file_exists(path: String) -> bool {
    PathBuf::from(path).exists()
}

#[tauri::command]
fn delete_file(path: String) -> Result<(), String> {
    let p = PathBuf::from(&path);
    if p.exists() {
        std::fs::remove_file(&p).map_err(|e| format!("删除文件失败: {e}"))?;
    }
    Ok(())
}

#[tauri::command]
async fn check_apk(app: tauri::AppHandle, path: String) -> ApkCheckResult {
    let apksigner = find_apksigner_path(&app);
    tauri::async_runtime::spawn_blocking(move || do_check_apk(path, apksigner))
        .await
        .unwrap_or_else(|e| ApkCheckResult {
            already_protected: false,
            is_signed: false,
            error: Some(format!("APK 检查任务异常: {e}")),
        })
}

#[tauri::command]
fn get_sign_config(app: tauri::AppHandle) -> Result<SignConfig, String> {
    load_sign_config(&app)
}

#[tauri::command]
fn save_sign_config(app: tauri::AppHandle, config: SignConfig) -> Result<(), String> {
    store_sign_config(&app, config)
}

#[tauri::command]
fn get_sign_passwords(app: tauri::AppHandle) -> Result<(String, String), String> {
    load_sign_passwords(&app)
}

#[tauri::command]
fn save_sign_passwords(
    app: tauri::AppHandle,
    keystore_password: String,
    key_password: String,
) -> Result<(), String> {
    store_sign_passwords(&app, keystore_password, key_password)
}

#[tauri::command]
async fn sign_apk(
    app: tauri::AppHandle,
    apk_path: String,
    output_path: Option<String>,
    apksigner_path: Option<String>,
    keystore_path: String,
    key_alias: String,
    ks_type: Option<String>,
    sign_v1: bool,
    sign_v2: bool,
    sign_v3: bool,
    sign_v4: bool,
) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        execute_sign_apk(
            &app,
            apk_path,
            output_path,
            apksigner_path,
            keystore_path,
            key_alias,
            ks_type,
            sign_v1,
            sign_v2,
            sign_v3,
            sign_v4,
        )
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
fn list_keystore_aliases(
    keystore_path: String,
    ks_pass: String,
    ks_type: Option<String>,
) -> Result<Vec<String>, String> {
    query_keystore_aliases(keystore_path, ks_pass, ks_type)
}

#[tauri::command]
fn get_locale(app: tauri::AppHandle) -> Result<String, String> {
    let store = app.store(STORE_FILE).map_err(|e| e.to_string())?;
    let locale = store
        .get(STORE_KEY_LOCALE)
        .and_then(|v| v.as_str().map(|s| s.to_string()))
        .unwrap_or_else(|| "zh".to_string());
    Ok(locale)
}

#[tauri::command]
fn save_locale(app: tauri::AppHandle, locale: String) -> Result<(), String> {
    let store = app.store(STORE_FILE).map_err(|e| e.to_string())?;
    store.set(STORE_KEY_LOCALE, serde_json::Value::String(locale));
    store.save().map_err(|e| e.to_string())
}

#[tauri::command]
fn get_home_dir(app: tauri::AppHandle) -> String {
    app.path()
        .home_dir()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct UpdateCheckResult {
    has_update: bool,
    latest_version: Option<String>,
    release_url: Option<String>,
    update_level: Option<String>,
}

fn compare_semver(current: &str, latest: &str, release_url: Option<String>) -> UpdateCheckResult {
    let no_update = || UpdateCheckResult {
        has_update: false,
        latest_version: None,
        release_url: None,
        update_level: None,
    };

    let current = match semver::Version::parse(current) {
        Ok(v) => v,
        Err(_) => return no_update(),
    };
    let latest_version = match semver::Version::parse(latest) {
        Ok(v) => v,
        Err(_) => return no_update(),
    };

    if latest_version <= current {
        return no_update();
    }

    let level = if latest_version.major > current.major {
        "major"
    } else if latest_version.minor > current.minor {
        "minor"
    } else {
        "patch"
    };

    UpdateCheckResult {
        has_update: true,
        latest_version: Some(latest.to_string()),
        release_url,
        update_level: Some(level.to_string()),
    }
}

fn get_cached_update(app: &tauri::AppHandle) -> Option<UpdateCheckResult> {
    let store = app.store(STORE_FILE).ok()?;
    let last_check = store.get(STORE_KEY_UPDATE_LAST_CHECK)?.as_i64()?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_secs() as i64;
    if now - last_check > 86400 {
        // 24 小时 = 86400 秒
        return None;
    }
    let latest_tag = store
        .get(STORE_KEY_UPDATE_LATEST_TAG)?
        .as_str()?
        .to_string();
    let release_url = store
        .get(STORE_KEY_UPDATE_RELEASE_URL)
        .and_then(|v| v.as_str().map(|s| s.to_string()));
    Some(compare_semver(
        env!("CARGO_PKG_VERSION"),
        &latest_tag,
        release_url,
    ))
}

fn save_update_to_cache(app: &tauri::AppHandle, latest_tag: &str, release_url: Option<&str>) {
    if let Ok(store) = app.store(STORE_FILE) {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        store.set(STORE_KEY_UPDATE_LAST_CHECK, serde_json::json!(now));
        store.set(
            STORE_KEY_UPDATE_LATEST_TAG,
            serde_json::Value::String(latest_tag.to_string()),
        );
        store.set(
            STORE_KEY_UPDATE_RELEASE_URL,
            release_url
                .map(|u| serde_json::Value::String(u.to_string()))
                .unwrap_or(serde_json::Value::Null),
        );
        let _ = store.save();
    }
}

#[tauri::command]
async fn check_update(app: tauri::AppHandle, force: bool) -> Result<UpdateCheckResult, String> {
    if !force {
        if let Some(cached) = get_cached_update(&app) {
            return Ok(cached);
        }
    }

    let current = env!("CARGO_PKG_VERSION");
    let user_agent = format!("mocika-shield/{}", current);

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|e| e.to_string())?;

    let response = client
        .get("https://api.github.com/repos/mocikadev/mocika-shield/releases/latest")
        .header("User-Agent", user_agent)
        .header("Accept", "application/vnd.github.v3+json")
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if response.status().as_u16() == 404 {
        // 尚无 Release，静默返回无更新
        save_update_to_cache(&app, "", None);
        return Ok(UpdateCheckResult {
            has_update: false,
            latest_version: None,
            release_url: None,
            update_level: None,
        });
    }

    if !response.status().is_success() {
        return Err(format!("GitHub API 返回错误状态码: {}", response.status()));
    }

    let json: serde_json::Value = response.json().await.map_err(|e| e.to_string())?;
    let tag = json["tag_name"].as_str().unwrap_or("");
    let latest = tag.trim_start_matches(|c: char| c == 'v' || c == 'V');
    let release_url = json["html_url"].as_str();

    save_update_to_cache(&app, latest, release_url);
    Ok(compare_semver(
        current,
        latest,
        release_url.map(|s| s.to_string()),
    ))
}

#[tauri::command]
fn open_url(url: String) -> Result<(), String> {
    #[cfg(target_os = "linux")]
    std::process::Command::new("xdg-open")
        .arg(&url)
        .spawn()
        .map_err(|e| e.to_string())?;
    #[cfg(target_os = "macos")]
    std::process::Command::new("open")
        .arg(&url)
        .spawn()
        .map_err(|e| e.to_string())?;
    #[cfg(target_os = "windows")]
    no_window_command("cmd")
        .args(["/c", "start", "", &url])
        .spawn()
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
fn dismiss_update(app: tauri::AppHandle, version: String) -> Result<(), String> {
    let store = app.store(STORE_FILE).map_err(|e| e.to_string())?;
    store.set(
        STORE_KEY_UPDATE_DISMISSED,
        serde_json::Value::String(version),
    );
    store.save().map_err(|e| e.to_string())
}

#[tauri::command]
fn get_dismissed_version(app: tauri::AppHandle) -> Result<Option<String>, String> {
    let store = app.store(STORE_FILE).map_err(|e| e.to_string())?;
    Ok(store
        .get(STORE_KEY_UPDATE_DISMISSED)
        .and_then(|v| v.as_str().map(|s| s.to_string())))
}

#[derive(Serialize)]
struct BuildInfo {
    apktool_version: String,
    apksigner_version: String,
}

#[derive(Serialize)]
struct AppInfo {
    version: String,
    git_hash: String,
    build_date: String,
}

#[tauri::command]
fn get_app_info() -> AppInfo {
    AppInfo {
        version: env!("CARGO_PKG_VERSION").to_string(),
        git_hash: option_env!("GIT_HASH").unwrap_or("dev").to_string(),
        build_date: option_env!("BUILD_DATE").unwrap_or("unknown").to_string(),
    }
}

#[tauri::command]
fn get_build_info(app: tauri::AppHandle) -> BuildInfo {
    let apktool_version = find_apktool_path(&app)
        .and_then(|jar| {
            let path_str = jar.to_str()?.to_string();
            no_window_command("java")
                .args(["-jar", &path_str, "--version"])
                .output()
                .ok()
                .filter(|o| o.status.success())
                .map(|o| {
                    String::from_utf8_lossy(&o.stdout)
                        .lines()
                        .next()
                        .unwrap_or("")
                        .trim()
                        .to_string()
                })
        })
        .unwrap_or_else(|| "未找到".to_string());

    let apksigner_version = find_apksigner_path(&app)
        .and_then(|jar| {
            let path_str = jar.to_str()?.to_string();
            no_window_command("java")
                .args(["-jar", &path_str, "--version"])
                .output()
                .ok()
                .filter(|o| o.status.success())
                .map(|o| {
                    String::from_utf8_lossy(&o.stdout)
                        .lines()
                        .next()
                        .unwrap_or("")
                        .trim()
                        .to_string()
                })
        })
        .unwrap_or_else(|| "未找到".to_string());

    BuildInfo {
        apktool_version,
        apksigner_version,
    }
}

fn main() {
    if let Err(err) = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_store::Builder::default().build())
        .manage(CancelHandle(Arc::new(AtomicBool::new(false))))
        .invoke_handler(tauri::generate_handler![
            protect_apk,
            cancel_protect,
            check_file_exists,
            delete_file,
            show_in_folder,
            check_apk,
            get_sign_config,
            save_sign_config,
            get_sign_passwords,
            save_sign_passwords,
            sign_apk,
            list_keystore_aliases,
            compare_cert_fingerprints,
            get_locale,
            save_locale,
            get_home_dir,
            check_update,
            open_url,
            dismiss_update,
            get_dismissed_version,
            get_app_info,
            get_build_info,
        ])
        .run(tauri::generate_context!())
    {
        panic!("启动 shield-gui 失败: {err}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn patch_update_detected() {
        let r = compare_semver("1.0.0", "1.0.1", Some("http://x".into()));
        assert!(r.has_update);
        assert_eq!(r.update_level.as_deref(), Some("patch"));
        assert_eq!(r.latest_version.as_deref(), Some("1.0.1"));
    }

    #[test]
    fn minor_update_detected() {
        let r = compare_semver("1.0.0", "1.1.0", Some("http://x".into()));
        assert!(r.has_update);
        assert_eq!(r.update_level.as_deref(), Some("minor"));
    }

    #[test]
    fn major_update_detected() {
        let r = compare_semver("1.0.0", "2.0.0", Some("http://x".into()));
        assert!(r.has_update);
        assert_eq!(r.update_level.as_deref(), Some("major"));
    }

    #[test]
    fn no_update_when_same_version() {
        let r = compare_semver("1.0.0", "1.0.0", None);
        assert!(!r.has_update);
        assert!(r.update_level.is_none());
    }

    #[test]
    fn no_update_when_current_is_newer() {
        let r = compare_semver("1.2.0", "1.0.5", None);
        assert!(!r.has_update);
    }

    #[test]
    fn no_update_on_invalid_latest() {
        let r = compare_semver("1.0.0", "not-a-version", None);
        assert!(!r.has_update);
    }

    #[test]
    fn no_update_on_empty_latest() {
        let r = compare_semver("1.0.0", "", None);
        assert!(!r.has_update);
    }

    #[test]
    fn v_prefix_stripped_before_compare() {
        let stripped = "v1.0.1".trim_start_matches(|c: char| c == 'v' || c == 'V');
        let r = compare_semver("1.0.0", stripped, None);
        assert!(r.has_update);
        assert_eq!(r.update_level.as_deref(), Some("patch"));
    }

    #[test]
    fn major_dominates_minor_patch() {
        let r = compare_semver("1.9.9", "2.0.0", None);
        assert!(r.has_update);
        assert_eq!(r.update_level.as_deref(), Some("major"));
    }

    #[test]
    fn release_url_preserved() {
        let url = "https://github.com/mocikadev/mocika-shield/releases/tag/v1.0.1";
        let r = compare_semver("1.0.0", "1.0.1", Some(url.into()));
        assert_eq!(r.release_url.as_deref(), Some(url));
    }

    #[test]
    fn stable_release_updates_release_candidate() {
        let r = compare_semver("1.2.0-rc.1", "1.2.0", None);
        assert!(r.has_update);
        assert_eq!(r.update_level.as_deref(), Some("patch"));
        assert_eq!(r.latest_version.as_deref(), Some("1.2.0"));
    }

    #[test]
    fn newer_release_candidate_updates_older_release_candidate() {
        let r = compare_semver("1.2.0-rc.1", "1.2.0-rc.2", None);
        assert!(r.has_update);
        assert_eq!(r.update_level.as_deref(), Some("patch"));
    }

    #[test]
    fn release_candidate_does_not_update_stable_release() {
        let r = compare_semver("1.2.0", "1.2.0-rc.2", None);
        assert!(!r.has_update);
    }

    #[test]
    fn minor_level_preserved_for_prerelease_current() {
        let r = compare_semver("1.2.0-rc.1", "1.3.0-rc.1", None);
        assert!(r.has_update);
        assert_eq!(r.update_level.as_deref(), Some("minor"));
    }
}

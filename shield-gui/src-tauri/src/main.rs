mod apk_check;
mod app_config;
mod app_paths;
mod build_info;
mod file_ops;
mod protect_runner;
mod signing;
mod updates;

use apk_check::{
    ApkCheckResult, CertCompareResult, do_check_apk, do_compare_cert_fingerprints,
};
use app_config::{
    AppConfigPayload, AppConfigState, load_app_config, normalize_locale,
    normalize_theme_mode, save_app_config_file,
};
use app_paths::find_apksigner_path;
use build_info::{
    AppInfo, BuildInfo, get_app_info as get_app_info_impl, get_build_info as get_build_info_impl,
};
use file_ops::{
    check_file_exists as check_file_exists_impl, delete_file as delete_file_impl,
    open_url as open_url_impl, show_in_folder as show_in_folder_impl,
};
use protect_runner::{CancelHandle, execute_protect_apk};
use signing::{execute_sign_apk, query_keystore_aliases};
use std::fs;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use tauri::Manager;
use updates::{UpdateCheckResult, check_update_impl};

#[tauri::command]
async fn compare_cert_fingerprints(
    app: tauri::AppHandle,
    apk_path: String,
    keystore_path: String,
    ks_pass: String,
    ks_type: Option<String>,
    key_alias: String,
) -> Result<CertCompareResult, String> {
    tokio::task::spawn_blocking(move || {
        Ok(do_compare_cert_fingerprints(
            apk_path,
            keystore_path,
            ks_pass,
            ks_type,
            key_alias,
            find_apksigner_path(&app),
        ))
    })
    .await
    .map_err(|err| format!("后台任务执行失败: {err}"))?
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
    show_in_folder_impl(path)
}

#[tauri::command]
fn check_file_exists(path: String) -> bool {
    check_file_exists_impl(path)
}

#[tauri::command]
fn delete_file(path: String) -> Result<(), String> {
    delete_file_impl(path)
}

#[tauri::command]
async fn check_apk(app: tauri::AppHandle, path: String) -> Result<ApkCheckResult, String> {
    tokio::task::spawn_blocking(move || Ok(do_check_apk(path, find_apksigner_path(&app))))
        .await
        .map_err(|err| format!("后台任务执行失败: {err}"))?
}

#[tauri::command]
fn get_app_config(state: tauri::State<'_, AppConfigState>) -> Result<AppConfigPayload, String> {
    let config = state.read()?;
    Ok(AppConfigPayload::from(&config))
}

#[tauri::command]
fn save_app_config(
    state: tauri::State<'_, AppConfigState>,
    config: AppConfigPayload,
) -> Result<(), String> {
    state.mutate(move |current| {
        current.locale = normalize_locale(&config.locale);
        current.theme_mode = normalize_theme_mode(&config.theme_mode);
        current.signing.apply_sign_config(config.sign_config);
        current.signing.keystore_password = config.keystore_password;
        current.signing.key_password = config.key_password;
    })
}

#[tauri::command]
async fn sign_apk(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppConfigState>,
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
    let config = state.read()?;
    let keystore_password = config.signing.keystore_password;
    let key_password = config.signing.key_password;

    tokio::task::spawn_blocking(move || {
        execute_sign_apk(
            &app,
            keystore_password,
            key_password,
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
    .map_err(|err| format!("后台任务执行失败: {err}"))?
}

#[tauri::command]
async fn list_keystore_aliases(
    keystore_path: String,
    ks_pass: String,
    ks_type: Option<String>,
) -> Result<Vec<String>, String> {
    tokio::task::spawn_blocking(move || query_keystore_aliases(keystore_path, ks_pass, ks_type))
        .await
        .map_err(|err| format!("后台任务执行失败: {err}"))?
}

#[tauri::command]
async fn check_update(
    state: tauri::State<'_, AppConfigState>,
    force: bool,
) -> Result<UpdateCheckResult, String> {
    check_update_impl(&state, force).await
}

#[tauri::command]
fn open_url(url: String) -> Result<(), String> {
    open_url_impl(url)
}

#[tauri::command]
fn dismiss_update(
    state: tauri::State<'_, AppConfigState>,
    version: String,
) -> Result<(), String> {
    state.mutate(move |config| {
        config.dismissed_version = if version.trim().is_empty() {
            None
        } else {
            Some(version)
        };
    })
}

#[tauri::command]
fn get_dismissed_version(
    state: tauri::State<'_, AppConfigState>,
) -> Result<Option<String>, String> {
    Ok(state.read()?.dismissed_version)
}

#[tauri::command]
fn get_app_info() -> AppInfo {
    get_app_info_impl()
}

#[tauri::command]
fn get_build_info(app: tauri::AppHandle) -> BuildInfo {
    get_build_info_impl(app)
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(CancelHandle(Arc::new(AtomicBool::new(false))))
        .setup(|app| {
            let (path, config, legacy_path) = load_app_config(app.handle())?;
            save_app_config_file(&path, &config)?;
            if let Some(legacy) = legacy_path {
                let _ = fs::remove_file(legacy);
            }
            app.manage(AppConfigState::new(path, config));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            compare_cert_fingerprints,
            protect_apk,
            cancel_protect,
            show_in_folder,
            check_file_exists,
            delete_file,
            check_apk,
            get_app_config,
            save_app_config,
            sign_apk,
            list_keystore_aliases,
            check_update,
            open_url,
            dismiss_update,
            get_dismissed_version,
            get_app_info,
            get_build_info
        ])
        .run(tauri::generate_context!())
        .unwrap_or_else(|err| panic!("启动 shield-gui 失败: {err}"));
}

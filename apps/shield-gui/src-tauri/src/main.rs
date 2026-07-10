#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod apk_check;
mod app_config;
mod app_paths;
mod build_info;
mod cert_service;
mod cert_store;
mod file_ops;
mod protect_runner;
mod signing;
mod updates;

use apk_check::{do_check_apk, do_compare_cert_fingerprints, ApkCheckResult, CertCompareResult};
use app_config::{
    load_app_config, normalize_locale, normalize_theme_mode, save_app_config_file,
    AppConfigPayload, AppConfigState,
};
use app_paths::find_apksigner_path;
use build_info::{
    get_app_info as get_app_info_impl, get_build_info as get_build_info_impl,
    get_diagnostic_info as get_diagnostic_info_impl, AppInfo, BuildInfo,
};
use cert_service::{
    create_managed_certificate, save_certificate_profile, validate_certificate_input,
    verify_saved_certificate,
};
use cert_store::{
    initialize_certificate_store, CertificateRecord, CertificateStoreState, CertificateUpsertInput,
    CertificateValidationInput, CertificateValidationResult, CreateManagedCertificateInput,
};
use file_ops::{
    check_file_exists as check_file_exists_impl, delete_file as delete_file_impl,
    open_url as open_url_impl, show_in_folder as show_in_folder_impl,
};
use protect_runner::{execute_protect_apk, CancelHandle};
use signing::{execute_sign_apk, query_keystore_aliases};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use tauri::Manager;
use updates::{check_update_impl, UpdateCheckResult};

#[tauri::command]
async fn compare_cert_fingerprints(
    app: tauri::AppHandle,
    state: tauri::State<'_, CertificateStoreState>,
    apk_path: String,
    certificate_id: String,
) -> Result<CertCompareResult, String> {
    let certificate = state
        .get_certificate(&certificate_id)?
        .ok_or_else(|| "未找到签名证书".to_string())?;
    tokio::task::spawn_blocking(move || {
        Ok(do_compare_cert_fingerprints(
            apk_path,
            certificate.keystore_path,
            certificate.keystore_password,
            Some(certificate.ks_type),
            certificate.key_alias,
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
    })
}

#[tauri::command]
async fn sign_apk(
    app: tauri::AppHandle,
    state: tauri::State<'_, CertificateStoreState>,
    apk_path: String,
    output_path: Option<String>,
    apksigner_path: Option<String>,
    certificate_id: String,
) -> Result<(), String> {
    let certificate = state
        .get_certificate(&certificate_id)?
        .ok_or_else(|| "未找到签名证书".to_string())?;
    tokio::task::spawn_blocking(move || {
        execute_sign_apk(&app, apk_path, output_path, apksigner_path, certificate)
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
fn list_certificates(
    state: tauri::State<'_, CertificateStoreState>,
) -> Result<Vec<CertificateRecord>, String> {
    state
        .list_certificates()
        .map(|items| items.into_iter().map(redact_certificate).collect())
}

#[tauri::command]
fn save_certificate(
    state: tauri::State<'_, CertificateStoreState>,
    input: CertificateUpsertInput,
) -> Result<CertificateRecord, String> {
    save_certificate_profile(&state, input).map(redact_certificate)
}

#[tauri::command]
fn validate_certificate(
    input: CertificateValidationInput,
) -> Result<CertificateValidationResult, String> {
    validate_certificate_input(input)
}

#[tauri::command]
fn set_default_certificate(
    state: tauri::State<'_, CertificateStoreState>,
    id: String,
) -> Result<(), String> {
    state.set_default_certificate(Some(&id))
}

#[tauri::command]
fn delete_certificate(
    state: tauri::State<'_, CertificateStoreState>,
    id: String,
    remove_keystore_file: bool,
) -> Result<Vec<CertificateRecord>, String> {
    state.delete_certificate(&id, remove_keystore_file)?;
    state
        .list_certificates()
        .map(|items| items.into_iter().map(redact_certificate).collect())
}

#[tauri::command]
fn verify_certificate(
    state: tauri::State<'_, CertificateStoreState>,
    id: String,
) -> Result<CertificateRecord, String> {
    verify_saved_certificate(&state, &id).map(redact_certificate)
}

#[tauri::command]
fn create_managed_certificate_command(
    state: tauri::State<'_, CertificateStoreState>,
    input: CreateManagedCertificateInput,
) -> Result<CertificateRecord, String> {
    create_managed_certificate(&state, input).map(redact_certificate)
}

fn redact_certificate(mut record: CertificateRecord) -> CertificateRecord {
    record.keystore_password.clear();
    record.key_password.clear();
    record
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
fn dismiss_update(state: tauri::State<'_, AppConfigState>, version: String) -> Result<(), String> {
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

#[tauri::command]
fn get_diagnostic_info(app: tauri::AppHandle) -> String {
    get_diagnostic_info_impl(app)
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(CancelHandle(Arc::new(AtomicBool::new(false))))
        .setup(|app| {
            let loaded = load_app_config(app.handle())?;
            save_app_config_file(&loaded.path, &loaded.config)?;
            let cert_store = initialize_certificate_store(app.handle())?;
            app.manage(AppConfigState::new(loaded.path, loaded.config));
            app.manage(cert_store);
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
            list_certificates,
            save_certificate,
            validate_certificate,
            set_default_certificate,
            delete_certificate,
            verify_certificate,
            create_managed_certificate_command,
            list_keystore_aliases,
            check_update,
            open_url,
            dismiss_update,
            get_dismissed_version,
            get_app_info,
            get_build_info,
            get_diagnostic_info
        ])
        .run(tauri::generate_context!())
        .unwrap_or_else(|err| panic!("启动 shield-gui 失败: {err}"));
}

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod apk_check;
mod app_config;
mod app_paths;
mod build_info;
mod cert_service;
mod cert_store;
mod file_ops;
mod manifest_inspect;
mod protect_runner;
mod signing;
mod task_manager;
mod telemetry;
mod updates;

use apk_check::{do_check_apk, do_compare_cert_fingerprints, ApkCheckResult, CertCompareResult};
use app_config::{
    load_app_config, normalize_locale, normalize_theme_mode, save_app_config_file,
    AppConfigPayload, AppConfigState, ProtectDefaults,
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
use protect_runner::{
    execute_protect_apk, CancelHandle, EnvironmentPolicyRequest, ProtectExecution, RuntimeMode,
};
use signing::{execute_sign_apk, query_keystore_aliases};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use task_manager::{TaskKind, TaskManager, TaskSnapshot, TaskStatus};
use tauri::Manager;
use updates::{check_update_impl, UpdateCheckResult};

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProtectRequest {
    task_id: String,
    input: String,
    output: String,
    signed_output: Option<String>,
    apktool_path: Option<String>,
    #[serde(default)]
    runtime_mode: RuntimeMode,
    #[serde(default)]
    environment_policy: EnvironmentPolicyRequest,
    certificate_id: Option<String>,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct SignRequest {
    task_id: String,
    apk_path: String,
    output_path: Option<String>,
    apksigner_path: Option<String>,
    certificate_id: String,
}

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
    telemetry_state: tauri::State<'_, AppConfigState>,
    certificate_state: tauri::State<'_, CertificateStoreState>,
    request: ProtectRequest,
    cancel_handle: tauri::State<'_, CancelHandle>,
    task_manager: tauri::State<'_, TaskManager>,
) -> Result<(), String> {
    let app = window.app_handle().clone();
    let signing_certificate = match request.certificate_id {
        Some(id) => Some(
            certificate_state
                .get_certificate(&id)?
                .ok_or_else(|| "未找到自动签名证书".to_string())?,
        ),
        None => None,
    };
    task_manager.begin(
        &window,
        request.task_id.clone(),
        TaskKind::Protect,
        request.input.clone(),
        request
            .signed_output
            .clone()
            .unwrap_or_else(|| request.output.clone()),
        "CheckTools",
    )?;
    telemetry::record_event(&telemetry_state, "protect_start_count");
    let result = execute_protect_apk(
        window.clone(),
        ProtectExecution {
            task_id: request.task_id.clone(),
            input: request.input,
            output: request.output,
            apktool_path: request.apktool_path,
            runtime_mode: request.runtime_mode,
            environment_policy: request.environment_policy,
            signing_certificate,
            signed_output: request.signed_output,
        },
        cancel_handle,
        task_manager.inner().clone(),
    )
    .await;
    let signing_started = task_manager
        .snapshot(&request.task_id)?
        .is_some_and(|task| {
            matches!(
                task.current_step.as_str(),
                "PrepareSign" | "AlignApk" | "SignApk" | "Cleanup"
            )
        });
    let status = match &result {
        Ok(()) => TaskStatus::Succeeded,
        Err(message) if message == "已取消" => TaskStatus::Cancelled,
        Err(_) => TaskStatus::Failed,
    };
    task_manager.finish(
        &window,
        &request.task_id,
        status,
        result.as_ref().err().cloned(),
    )?;
    telemetry::record_event(
        &telemetry_state,
        if result.is_ok() {
            "protect_success_count"
        } else {
            "protect_failed_count"
        },
    );
    if signing_started {
        telemetry::record_event(
            &telemetry_state,
            if result.is_ok() {
                "sign_success_count"
            } else {
                "sign_failed_count"
            },
        );
    }
    telemetry::schedule_sync(app);
    result
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
async fn check_apk(
    app: tauri::AppHandle,
    state: tauri::State<'_, CertificateStoreState>,
    path: String,
    runtime_mode: RuntimeMode,
    certificate_id: Option<String>,
) -> Result<ApkCheckResult, String> {
    let certificate = certificate_id
        .map(|id| {
            state
                .get_certificate(&id)?
                .ok_or_else(|| "未找到自动签名证书".to_string())
        })
        .transpose()?;
    tokio::task::spawn_blocking(move || {
        Ok(do_check_apk(
            path,
            find_apksigner_path(&app),
            runtime_mode.preflight_profile(),
            certificate,
        ))
    })
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
        current.telemetry.enabled = config.telemetry_enabled;
        if let Some(defaults) = config.protect_defaults {
            current.protect_defaults = defaults;
        }
    })
}

#[tauri::command]
fn save_protect_defaults(
    state: tauri::State<'_, AppConfigState>,
    defaults: ProtectDefaults,
) -> Result<(), String> {
    state.mutate(move |current| {
        current.protect_defaults = defaults;
    })
}

#[tauri::command]
async fn sign_apk(
    window: tauri::Window,
    telemetry_state: tauri::State<'_, AppConfigState>,
    state: tauri::State<'_, CertificateStoreState>,
    request: SignRequest,
    task_manager: tauri::State<'_, TaskManager>,
) -> Result<(), String> {
    let app = window.app_handle().clone();
    let telemetry_app = app.clone();
    let certificate = state
        .get_certificate(&request.certificate_id)?
        .ok_or_else(|| "未找到签名证书".to_string())?;
    let final_output = request
        .output_path
        .clone()
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| request.apk_path.clone());
    task_manager.begin(
        &window,
        request.task_id.clone(),
        TaskKind::Sign,
        request.apk_path.clone(),
        final_output,
        "PrepareSign",
    )?;
    let progress_manager = task_manager.inner().clone();
    let progress_window = window.clone();
    let progress_task_id = request.task_id.clone();
    let result = tokio::task::spawn_blocking(move || {
        execute_sign_apk(
            &app,
            request.apk_path,
            request.output_path,
            request.apksigner_path,
            certificate,
            |step, message| {
                progress_manager.progress(&progress_window, &progress_task_id, step, message)
            },
        )
    })
    .await
    .unwrap_or_else(|err| Err(format!("后台任务执行失败: {err}")));
    task_manager.finish(
        &window,
        &request.task_id,
        if result.is_ok() {
            TaskStatus::Succeeded
        } else {
            TaskStatus::Failed
        },
        result.as_ref().err().cloned(),
    )?;
    telemetry::record_event(
        &telemetry_state,
        if result.is_ok() {
            "sign_success_count"
        } else {
            "sign_failed_count"
        },
    );
    telemetry::schedule_sync(telemetry_app);
    result
}

#[tauri::command]
fn get_latest_task(
    state: tauri::State<'_, TaskManager>,
    kind: String,
) -> Result<Option<TaskSnapshot>, String> {
    state.latest(if kind == "sign" {
        TaskKind::Sign
    } else {
        TaskKind::Protect
    })
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
async fn sync_telemetry(state: tauri::State<'_, AppConfigState>) -> Result<(), String> {
    telemetry::sync_pending(&state).await;
    Ok(())
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
        .manage(telemetry::TelemetryRuntime::default())
        .manage(TaskManager::default())
        .setup(|app| {
            let loaded = load_app_config(app.handle())?;
            save_app_config_file(&loaded.path, &loaded.config)?;
            let cert_store = initialize_certificate_store(app.handle())?;
            let config_state = AppConfigState::new(loaded.path, loaded.config);
            telemetry::record_app_start(&config_state);
            app.manage(config_state);
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
            save_protect_defaults,
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
            sync_telemetry,
            open_url,
            dismiss_update,
            get_dismissed_version,
            get_app_info,
            get_build_info,
            get_diagnostic_info,
            get_latest_task
        ])
        .run(tauri::generate_context!())
        .unwrap_or_else(|err| panic!("启动 shield-gui 失败: {err}"));
}

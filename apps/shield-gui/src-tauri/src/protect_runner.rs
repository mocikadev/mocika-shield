use crate::app_paths::{
    find_apksigner_path, find_apktool_path, find_resources_path, strip_unc_prefix,
};
use crate::cert_store::CertificateRecord;
use crate::signing::execute_sign_apk;
use crate::task_manager::TaskManager;
use shield_core::{
    extract_keystore_cert_fingerprint, protect_apk as shield_protect_apk, ProgressEvent,
    ProtectOptions, ShieldError,
};
use std::path::PathBuf;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use tauri::Manager;

pub(crate) struct CancelHandle(pub Arc<AtomicBool>);

pub(crate) struct ProtectExecution {
    pub(crate) task_id: String,
    pub(crate) input: String,
    pub(crate) output: String,
    pub(crate) apktool_path: Option<String>,
    pub(crate) resources_path: Option<String>,
    pub(crate) signing_certificate: Option<CertificateRecord>,
    pub(crate) signed_output: Option<String>,
}

pub(crate) async fn execute_protect_apk(
    window: tauri::Window,
    request: ProtectExecution,
    cancel_handle: tauri::State<'_, CancelHandle>,
    task_manager: TaskManager,
) -> Result<(), String> {
    cancel_handle.inner().0.store(false, Ordering::SeqCst);

    let app = window.app_handle().clone();

    let resolved_apktool = request
        .apktool_path
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .or_else(|| find_apktool_path(&app));

    let resolved_resources = request
        .resources_path
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .or_else(|| find_resources_path(&app));
    let resolved_apksigner = find_apksigner_path(&app);

    let cancel = Arc::clone(&cancel_handle.inner().0);
    let emit_error = Arc::new(Mutex::new(None::<String>));
    let emit_error_for_task = Arc::clone(&emit_error);
    let progress_window = window.clone();
    let progress_task_id = request.task_id;

    let task_result = tokio::task::spawn_blocking(move || {
        let expected_output_cert_fingerprint = request
            .signing_certificate
            .as_ref()
            .map(|certificate| {
                extract_keystore_cert_fingerprint(
                    std::path::Path::new(&certificate.keystore_path),
                    &certificate.key_alias,
                    &certificate.keystore_password,
                    Some(&certificate.ks_type),
                )
            })
            .transpose()
            .map_err(ShieldError::from)?;
        let opts = ProtectOptions {
            input: strip_unc_prefix(PathBuf::from(request.input)),
            output: strip_unc_prefix(PathBuf::from(request.output)),
            apktool_path: resolved_apktool,
            resources_path: resolved_resources,
            apksigner_path: resolved_apksigner,
            expected_output_cert_fingerprint,
        };
        let cancel_for_progress = Arc::clone(&cancel);
        let cancel_for_protect = Arc::clone(&cancel);
        let protect_manager = task_manager.clone();
        let protect_window = progress_window.clone();
        let protect_task_id = progress_task_id.clone();

        shield_protect_apk(
            &opts,
            move |event: ProgressEvent| {
                if let Err(err) = protect_manager.progress(
                    &protect_window,
                    &protect_task_id,
                    &format!("{:?}", event.step),
                    event.message,
                ) {
                    if let Ok(mut slot) = emit_error_for_task.lock() {
                        if slot.is_none() {
                            *slot = Some(format!("发送进度事件失败: {err}"));
                        }
                    }
                    cancel_for_progress.store(true, Ordering::SeqCst);
                }
            },
            cancel_for_protect,
        )?;

        if let (Some(certificate), Some(final_output)) =
            (request.signing_certificate, request.signed_output)
        {
            if cancel.load(Ordering::Relaxed) {
                return Err(ShieldError::Cancelled);
            }
            let unsigned_output = opts.output.to_string_lossy().to_string();
            execute_sign_apk(
                &app,
                unsigned_output.clone(),
                Some(final_output.clone()),
                None,
                certificate,
                |step, message| {
                    task_manager.progress(&progress_window, &progress_task_id, step, message)
                },
            )
            .map_err(ShieldError::ApkError)?;
            let _ = std::fs::remove_file(format!("{final_output}.idsig"));
            let _ = std::fs::remove_file(unsigned_output);
            task_manager
                .progress(
                    &progress_window,
                    &progress_task_id,
                    "Cleanup",
                    "已清理中间产物",
                )
                .map_err(ShieldError::ApkError)?;
        }
        Ok(())
    })
    .await
    .map_err(|err| format!("后台任务执行失败: {err}"))?;

    let progress_emit_error = emit_error
        .lock()
        .map_err(|_| "发送进度事件失败：状态锁已损坏".to_string())?
        .clone();

    if let Some(msg) = progress_emit_error {
        return Err(msg);
    }

    match task_result {
        Ok(()) => Ok(()),
        Err(ShieldError::Cancelled) => Err("已取消".to_string()),
        Err(err) => Err(err.to_string()),
    }
}

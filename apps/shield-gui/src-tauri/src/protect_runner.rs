use crate::app_paths::{
    find_apksigner_path, find_apktool_path, find_named_resources_path, find_resources_path,
    strip_unc_prefix,
};
use crate::cert_store::CertificateRecord;
use crate::failure_diagnostic::ExecutionFailure;
use crate::signing::execute_sign_apk;
use crate::task_manager::TaskManager;
use shield_core::diagnostic::Diagnostic;
use shield_core::{
    extract_keystore_cert_fingerprint, protect_apk as shield_protect_apk, EnvironmentPolicy,
    ProgressEvent, ProtectOptions, ShieldError,
};
use std::path::PathBuf;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use tauri::Manager;

pub(crate) struct CancelHandle(pub Arc<AtomicBool>);

#[derive(Clone, Copy, Default, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum RuntimeMode {
    #[default]
    Standard,
    AndroidApi19,
}

impl RuntimeMode {
    pub(crate) fn preflight_profile(self) -> shield_core::RuntimeProfile {
        match self {
            Self::Standard => shield_core::RuntimeProfile::Standard,
            Self::AndroidApi19 => shield_core::RuntimeProfile::AndroidApi19,
        }
    }
}

#[derive(Clone, Copy, Default, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum EnvironmentPolicyRequest {
    #[default]
    Compatible,
    Strict,
}

pub(crate) struct ProtectExecution {
    pub(crate) excluded_abis: Vec<String>,
    pub(crate) task_id: String,
    pub(crate) input: String,
    pub(crate) output: String,
    pub(crate) apktool_path: Option<String>,
    pub(crate) runtime_mode: RuntimeMode,
    pub(crate) environment_policy: EnvironmentPolicyRequest,
    pub(crate) signing_certificate: Option<CertificateRecord>,
    pub(crate) signed_output: Option<String>,
}

pub(crate) async fn execute_protect_apk(
    window: tauri::Window,
    request: ProtectExecution,
    cancel_handle: tauri::State<'_, CancelHandle>,
    task_manager: TaskManager,
) -> Result<(), ExecutionFailure> {
    cancel_handle.inner().0.store(false, Ordering::SeqCst);

    let app = window.app_handle().clone();

    let resolved_apktool = request
        .apktool_path
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .or_else(|| find_apktool_path(&app));

    let resolved_resources = match request.runtime_mode {
        RuntimeMode::Standard => find_resources_path(&app),
        RuntimeMode::AndroidApi19 => {
            let checked = shield_core::check_apk(
                std::path::Path::new(&request.input),
                find_apksigner_path(&app).as_deref(),
            )
            .map_err(|err| format!("检查 Android 4.4 兼容条件失败：{err}"))?;
            request
                .runtime_mode
                .preflight_profile()
                .validate_native_abis(&checked.native_abis, &request.excluded_abis)
                .map_err(|err| {
                    ExecutionFailure::preflight(err.to_string(), Diagnostic::from_error(&err))
                })?;
            Some(
                find_named_resources_path(&app, "resources-api19.zip").ok_or_else(|| {
                    ExecutionFailure::preflight(
                        "缺少 Android 4.4 兼容资源 resources-api19.zip，请重新安装完整应用包"
                            .into(),
                        Diagnostic::new(shield_core::diagnostic::FailureCode::ToolNotFound),
                    )
                })?,
            )
        }
    };
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
            .map_err(|err| {
                ExecutionFailure::diagnosed(err.to_string(), Diagnostic::from_error(&err))
            })?;
        let opts = ProtectOptions {
            excluded_abis: request.excluded_abis,
            input: strip_unc_prefix(PathBuf::from(request.input)),
            output: strip_unc_prefix(PathBuf::from(request.output)),
            apktool_path: resolved_apktool,
            resources_path: resolved_resources,
            apksigner_path: resolved_apksigner,
            expected_output_cert_fingerprint,
            environment_policy: match request.environment_policy {
                EnvironmentPolicyRequest::Compatible => EnvironmentPolicy::Compatible,
                EnvironmentPolicyRequest::Strict => EnvironmentPolicy::Strict,
            },
        };
        let cancel_for_progress = Arc::clone(&cancel);
        let cancel_for_protect = Arc::clone(&cancel);
        let protect_manager = task_manager.clone();
        let protect_window = progress_window.clone();
        let protect_task_id = progress_task_id.clone();

        app.state::<crate::application_sharing::SharingState>()
            .verify_input(&progress_task_id);
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
        )
        .map_err(|source| {
            let mut failure = ExecutionFailure::from_shield(source);
            failure.tool_path = match failure.diagnostic.tool {
                Some(shield_core::diagnostic::ToolName::Apktool) => opts.apktool_path.clone(),
                Some(shield_core::diagnostic::ToolName::Apksigner) => opts.apksigner_path.clone(),
                _ => None,
            };
            failure
        })?;

        let sharing = app.state::<crate::application_sharing::SharingState>();
        sharing.protected(&progress_task_id);
        sharing.verify_input(&progress_task_id);

        task_manager.protected(
            &progress_task_id,
            request.signing_certificate.is_some() && request.signed_output.is_some(),
        )?;

        if let (Some(certificate), Some(final_output)) =
            (request.signing_certificate, request.signed_output)
        {
            if cancel.load(Ordering::Relaxed) {
                return Err(ExecutionFailure::from_shield(ShieldError::Cancelled));
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
            )?;
            let _ = std::fs::remove_file(format!("{final_output}.idsig"));
            let _ = std::fs::remove_file(unsigned_output);
            task_manager.progress(
                &progress_window,
                &progress_task_id,
                "Cleanup",
                "已清理中间产物",
            )?;
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
        return Err(msg.into());
    }

    task_result
}

#[cfg(test)]
mod tests {
    use super::RuntimeMode;

    #[test]
    fn 工控模式复用核心预检配置() {
        assert_eq!(
            RuntimeMode::AndroidApi19.preflight_profile(),
            shield_core::RuntimeProfile::AndroidApi19
        );
        assert_eq!(
            RuntimeMode::Standard.preflight_profile(),
            shield_core::RuntimeProfile::Standard
        );
    }

    #[test]
    fn 工控混合架构任务仍须确认排除旧架构() {
        let abis = ["armeabi-v7a", "arm64-v8a", "x86", "x86_64", "armeabi"].map(String::from);
        let profile = RuntimeMode::AndroidApi19.preflight_profile();
        assert!(profile.validate_native_abis(&abis, &[]).is_err());
        assert!(profile
            .validate_native_abis(&abis, &["armeabi".into()])
            .is_ok());
    }
}

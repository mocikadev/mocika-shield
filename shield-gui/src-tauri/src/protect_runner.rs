use crate::app_paths::{find_apktool_path, find_resources_path, strip_unc_prefix};
use serde::Serialize;
use shield_cli::{protect_apk as shield_protect_apk, ProgressEvent, ProtectOptions, ShieldError};
use std::path::PathBuf;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use tauri::Emitter;
use tauri::Manager;

pub(crate) struct CancelHandle(pub Arc<AtomicBool>);

#[derive(Debug, Clone, Serialize)]
struct ProtectProgressPayload {
    step: String,
    message: String,
}

pub(crate) async fn execute_protect_apk(
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

//! 任务终态的单次计数和诊断捕获；界面错误保持原文，仅安全结构进入报告。
use crate::app_config::AppConfigState;
use crate::cert_service::CertificateOutcome;
use crate::error_report::{now_seconds, ErrorReportState};
use crate::failure_diagnostic::{ExecutionFailure, FailureContext, SafeReport};
use crate::task_manager::{TaskKind, TaskManager, TaskStatus};
use crate::telemetry::{self, TelemetryEvent};
use tauri::{Emitter, Manager};

pub(crate) fn finish_task(
    window: &tauri::Window,
    manager: &TaskManager,
    config: &AppConfigState,
    task_id: &str,
    auto_sign: bool,
    result: Result<(), ExecutionFailure>,
) -> Result<(), String> {
    let task = manager
        .snapshot(task_id)?
        .ok_or_else(|| "未找到任务状态".to_string())?;
    let status = match &result {
        Ok(()) => TaskStatus::Succeeded,
        Err(error) if error.cancelled => TaskStatus::Cancelled,
        Err(_) => TaskStatus::Failed,
    };
    if manager.finish(
        window,
        task_id,
        status,
        result.as_ref().err().map(|error| error.message.clone()),
    )? {
        window
            .state::<crate::application_sharing::SharingState>()
            .finish(
                window.app_handle().clone(),
                config,
                task_id,
                task.kind,
                status,
                task.protect_succeeded,
            );
        let protected =
            task.kind == TaskKind::Protect && (task.protect_succeeded || result.is_ok());
        if protected {
            telemetry::record_event(config, TelemetryEvent::ProtectSucceeded);
        }
        if result.is_ok() && (task.kind == TaskKind::Sign || task.signing_started) {
            telemetry::record_event(config, TelemetryEvent::SignSucceeded);
        }
        if let Err(error) = &result {
            if is_executed_failure(error) {
                let (stage, context) = if task.kind == TaskKind::Protect {
                    (
                        telemetry::protect_failure_stage(&task.current_step, task.signing_started),
                        FailureContext::protect(
                            &task.current_step,
                            task.signing_started,
                            auto_sign,
                        ),
                    )
                } else {
                    (
                        telemetry::sign_failure_stage(&task.current_step),
                        FailureContext::sign(&task.current_step),
                    )
                };
                telemetry::record_failure(config, stage);
                capture_report(
                    window.app_handle(),
                    config,
                    context,
                    error,
                    Some((&task.input_path, &task.output_path, &task.current_step)),
                );
            }
        }
        telemetry::schedule_sync(window.app_handle().clone());
    }
    result.map_err(|error| error.message)
}

pub(crate) fn capture_failure(
    app: &tauri::AppHandle,
    config: &AppConfigState,
    context: FailureContext,
    error: &ExecutionFailure,
) {
    capture_report(app, config, context, error, None);
}

fn capture_report(
    app: &tauri::AppHandle,
    config: &AppConfigState,
    context: FailureContext,
    error: &ExecutionFailure,
    paths: Option<(&str, &str, &str)>,
) {
    if !is_executed_failure(error) {
        return;
    }
    telemetry::record_reason(config, context, *error.diagnostic);
    let mut report = SafeReport::new(context, *error.diagnostic, now_seconds());
    if let Some((input, output, step)) = paths {
        report.details.input_path = Some(crate::report_context::PathFeatures::of(input));
        report.details.output_path = Some(crate::report_context::PathFeatures::of(output));
        report.details.execution_step = crate::report_context::execution_step(step);
    }
    let app = app.clone();
    let tool_path = error.tool_path.clone();
    let prompt = error.diagnostic.code.should_prompt();
    // 原任务先返回；后台探测只持有有限报告与工具路径，不持有错误正文或证书材料。
    tauri::async_runtime::spawn_blocking(move || {
        crate::report_context::enrich(&mut report, tool_path.as_deref());
        if let Ok(preview) = app.state::<ErrorReportState>().capture(report, prompt) {
            let _ = app.emit("failure-diagnostic", preview);
        }
    });
}

pub(crate) fn certificate_result<T>(
    app: &tauri::AppHandle,
    result: Result<T, ExecutionFailure>,
) -> Result<T, String> {
    if let Err(error) = &result {
        if is_executed_failure(error) {
            capture_failure(
                app,
                &app.state::<AppConfigState>(),
                FailureContext::certificate(),
                error,
            );
            telemetry::schedule_sync(app.clone());
        }
    }
    result.map_err(|error| error.message)
}

fn is_executed_failure(error: &ExecutionFailure) -> bool {
    error.executed && !error.cancelled
}

#[cfg(test)]
mod tests {
    use super::is_executed_failure;
    use crate::failure_diagnostic::ExecutionFailure;
    use shield_core::diagnostic::{Diagnostic, FailureCode};

    #[test]
    fn 只有实际执行失败才进入统计与报告通道() {
        let preflight = ExecutionFailure::preflight(
            "APK 预检未通过".to_string(),
            Diagnostic::new(FailureCode::UnsupportedAbi),
        );
        let runtime = ExecutionFailure::from("发送进度事件失败".to_string());
        let cancelled = ExecutionFailure::from_shield(shield_core::ShieldError::Cancelled);

        assert!(!is_executed_failure(&preflight));
        assert!(is_executed_failure(&runtime));
        assert!(!is_executed_failure(&cancelled));
    }
}

pub(crate) fn certificate_outcome<T>(
    app: &tauri::AppHandle,
    result: Result<CertificateOutcome<T>, ExecutionFailure>,
) -> Result<T, String> {
    match result {
        Ok(outcome) => {
            if let Some(diagnostic) = outcome.diagnostic {
                let failure = ExecutionFailure::diagnosed("证书校验失败".to_string(), diagnostic);
                capture_failure(
                    app,
                    &app.state::<AppConfigState>(),
                    FailureContext::certificate(),
                    &failure,
                );
                telemetry::schedule_sync(app.clone());
            }
            Ok(outcome.value)
        }
        Err(error) => certificate_result(app, Err(error)),
    }
}

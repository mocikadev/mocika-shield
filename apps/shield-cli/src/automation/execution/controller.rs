//! 父端开发验证编排：全部 Java 在隔离工作进程内，父端只核对字节/身份并无覆盖提交。
use super::identity::Tools;
use super::launch::{LaunchError, Launcher};
use super::*;
use crate::automation::{
    publication::{Publication, PublicationCheckpoint, PublicationError},
    snapshot,
    task_lifecycle::{Output, Phase, Status, Task},
    worker_protocol::{Message, Rejection},
};
use serde_json::{json, Value};
use std::process::Command;
use std::time::Duration;

#[cfg(test)]
#[path = "publication_guard_tests.rs"]
mod publication_guard_tests;
#[cfg(all(test, unix))]
#[path = "signing_tests.rs"]
mod signing_tests;
#[cfg(test)]
mod startup_tests;
#[cfg(all(test, unix))]
#[path = "../../../tests/support/execution_fixture.rs"]
mod test_fixture;
#[cfg(all(test, unix))]
#[path = "controller_tests.rs"]
mod tests;

struct Planned {
    digest: [u8; 32],
    certificate: [u8; 32],
    sources: sources::Sources,
    tools: Tools,
    warning: bool,
    cleanup_complete: bool,
}

fn rejection(code: Rejection) -> CheckError {
    error(match code {
        Rejection::V4Unsupported => "SIGNING_V4_NOT_SUPPORTED",
        Rejection::PlanBlocked => "PLAN_BLOCKED",
        Rejection::PlanMismatch => "PLAN_MISMATCH",
        Rejection::SourceChanged => "PLAN_SOURCE_CHANGED",
        Rejection::InvalidRequest => "INVALID_REQUEST",
        Rejection::CredentialUnavailable => "CREDENTIAL_UNAVAILABLE",
        Rejection::ValidationFailed => "ARTIFACT_INVALID",
        Rejection::ProtectionMissing => "PROTECTION_MISSING",
        Rejection::SignatureInvalid => "SIGNATURE_INVALID",
        Rejection::CertificateMismatch => "CERTIFICATE_MISMATCH",
        Rejection::AlignmentInvalid => "ALIGNMENT_INVALID",
        Rejection::MaterialInvalid => "MATERIAL_INVALID",
        Rejection::ExecutorChanged => "EXECUTOR_CHANGED",
        Rejection::ToolchainChanged => "TOOLCHAIN_CHANGED",
        Rejection::SigningInputInvalid => "SIGNING_INPUT_INVALID",
        Rejection::SigningVersionsInvalid => "SIGNING_VERSIONS_INVALID",
        Rejection::SigningConfigInvalid => "SIGNING_CONFIG_INVALID",
    })
}

fn planning(
    request: &Request,
    deadline: Instant,
    cancel: &AtomicBool,
    launcher: &mut Launcher<impl FnMut() -> Command>,
) -> Result<Planned, CheckError> {
    checkpoint(deadline, cancel).map_err(|e| e.with_cleanup(true))?;
    let target = snapshot::output(&request.output)?;
    let parent = target.parent().ok_or_else(|| error("INVALID_REQUEST"))?;
    let directory = crate::automation::task_files::open_directory(parent)
        .map_err(|_| error("WORKER_START_FAILED"))?;
    let root = crate::automation::task_files::WorkDirectory::new(parent, &directory)
        .map_err(|_| error("WORKER_START_FAILED"))?;
    let (mut group, pump) =
        match launcher.spawn(request, Mode::Plan, root.path(), None, deadline, cancel) {
            Ok(started) => started,
            Err(LaunchError::Inactive(error)) => {
                return Err(error.with_cleanup(root.close().is_ok()))
            }
            Err(LaunchError::Unconfirmed) => {
                let _ = root.keep();
                return Err(error("STOP_UNCONFIRMED"));
            }
        };
    let result = (|| {
        let mut hello = false;
        let mut stage = None;
        loop {
            checkpoint(deadline, cancel)?;
            match pump.next()? {
                Some(Message::Hello {}) if !hello => {
                    hello = true;
                    launcher.recheck(deadline, cancel)?;
                    group.start([0; 32]).map_err(|_| error("PROTOCOL_FAILED"))?;
                }
                Some(Message::Progress { stage: next })
                    if hello && stage.is_none_or(|previous| next > previous) =>
                {
                    stage = Some(next)
                }
                Some(Message::Planned {
                    plan_digest,
                    certificate_sha256,
                    sources_sha256,
                }) if hello => return Ok((plan_digest, certificate_sha256, sources_sha256)),
                Some(Message::Rejected { code }) => return Err(rejection(code)),
                Some(Message::Failed { code }) => return Err(error(code.as_str())),
                Some(_) => return Err(error("PROTOCOL_FAILED")),
                None => {
                    if group
                        .leader_exited()
                        .map_err(|_| error("STOP_UNCONFIRMED"))?
                    {
                        return Err(error("WORKER_EXITED"));
                    }
                }
            }
        }
    })();
    let stopped = group.stop(Duration::from_secs(2)).is_ok();
    let channel_finished = pump.finish().is_ok_and(|valid| valid);
    if !stopped || !channel_finished {
        let _ = root.keep();
        return Err(error("STOP_UNCONFIRMED"));
    }
    // 已确认整组和管道停止后，仍须显式关闭临时目录才能对外确认清理。
    let prepared = (|| {
        let (digest, certificate, expected_sources) = result?;
        let (proof, bytes) = read_json::<PlanProof>(&root.path().join("sources.json"), 65536)?;
        if <[u8; 32]>::from(Sha256::digest(bytes)) != expected_sources {
            return Err(error("SOURCE_PROOF_INVALID"));
        }
        if proof.format_version != request.proof_version() {
            return Err(error("SOURCE_PROOF_INVALID"));
        }
        let PlanProof {
            sources, warning, ..
        } = proof;
        sources.matches(request)?;
        sources.recheck(deadline, cancel)?;
        launcher.recheck(deadline, cancel)?;
        let tools = match &sources {
            sources::Sources::Protect(s) => {
                Tools::capture(s, &mut || checkpoint(deadline, cancel))?
            }
            sources::Sources::Sign(_) => {
                let (java, keytool) = sources.tools();
                Tools::capture_files(java, keytool, &mut || checkpoint(deadline, cancel))?
            }
        };
        Ok(Planned {
            digest,
            certificate,
            sources,
            tools,
            warning,
            cleanup_complete: false,
        })
    })();
    let complete = root.close().is_ok();
    prepared
        .map(|mut planned| {
            planned.cleanup_complete = complete;
            planned
        })
        .map_err(|error: CheckError| error.with_cleanup(complete))
}

pub(super) fn run(request: Request, cancel: Arc<AtomicBool>) -> Result<Value, CheckError> {
    run_controlled(request, cancel, RunMode::Probe, |_| {})
}

pub(super) fn run_controlled(
    request: Request,
    cancel: Arc<AtomicBool>,
    mode: RunMode,
    notice: impl FnMut(Notice),
) -> Result<Value, CheckError> {
    let program = std::env::current_exe().map_err(|_| error("WORKER_START_FAILED"))?;
    run_with_options(
        request,
        cancel,
        || shield_core::utils::no_window_command(&program),
        mode,
        notice,
    )
}

// 只隔离已有子进程的构造，供测试选择显式构建的同一 CLI 并设置局部环境。
// 请求、配置及正式命令均不能传入程序或参数，平台控制仍由 launch 独占。
#[cfg(all(test, unix))]
fn run_with_command(
    request: Request,
    cancel: Arc<AtomicBool>,
    command: impl FnMut() -> Command,
) -> Result<Value, CheckError> {
    run_with_options(request, cancel, command, RunMode::Probe, |_| {})
}

fn run_with_options(
    mut request: Request,
    cancel: Arc<AtomicBool>,
    command: impl FnMut() -> Command,
    mode: RunMode,
    mut notice: impl FnMut(Notice),
) -> Result<Value, CheckError> {
    let started = Instant::now();
    let timeout = Duration::from_secs(request.timeout_seconds);
    let deadline = started
        .checked_add(timeout)
        .ok_or_else(|| error("INVALID_REQUEST"))?;
    let mut launcher = Launcher::new(command);
    notice(Notice::Stage(
        crate::automation::task_lifecycle::Stage::Prepare,
    ));
    let planned = planning(&request, deadline, &cancel, &mut launcher)?;
    // 后续进程使用已固定身份，不再次选择默认项。
    planned.sources.bind_request(&mut request);
    checkpoint(deadline, &cancel).map_err(|e| e.with_cleanup(planned.cleanup_complete))?;
    if let RunMode::Protect(expected) = mode {
        if expected != planned.digest {
            return Err(error("PLAN_MISMATCH").with_cleanup(planned.cleanup_complete));
        }
    }
    if matches!(mode, RunMode::Plan) {
        launcher
            .verify(deadline, &cancel)
            .map_err(|e| e.with_cleanup(planned.cleanup_complete))?;
        checkpoint(deadline, &cancel).map_err(|e| e.with_cleanup(planned.cleanup_complete))?;
        let mut value = json!({"type":"result","status":if planned.warning { "warning" } else { "ready" },
            "output_state":"not_published","cleanup_complete":planned.cleanup_complete,"retryable":false,
            "elapsed_ms":started.elapsed().as_millis(),"plan_digest":hex(&planned.digest)});
        if let Some(binding) = planned.sources.shared_binding()? {
            value["shared_binding"] = json!(binding);
        }
        if request.sign.is_some() {
            value["task_kind"] = json!("sign");
        }
        return Ok(value);
    }
    let mut publication =
        Publication::new(&request.output).map_err(|_| error("OUTPUT_UNAVAILABLE"))?;
    let root = publication
        .work_dir()
        .map_err(|_| error("OUTPUT_UNAVAILABLE"))?
        .to_owned();
    let mut task = match planned.sources.content() {
        Some(content) => Task::new_sign(
            started,
            timeout,
            planned.digest,
            planned.certificate,
            content,
        ),
        None => Task::new(started, timeout, planned.digest, planned.certificate),
    }
    .map_err(|_| error("INVALID_REQUEST"))?;
    let launched = launcher.spawn(
        &request,
        Mode::Execute,
        &root,
        Some(&planned.tools),
        deadline,
        &cancel,
    );
    let (mut group, pump) = match launched {
        Ok(started) => started,
        Err(LaunchError::Inactive(error)) => {
            return Err(error.with_cleanup(publication.cleanup()));
        }
        Err(LaunchError::Unconfirmed) => {
            publication.preserve();
            return Err(error("STOP_UNCONFIRMED"));
        }
    };
    if task.isolation_confirmed(Instant::now()).is_err() {
        // 启动期间耗尽期限也必须显式收尾，不能依赖进程析构后就清理活动目录。
        let stopped = group.stop(Duration::from_secs(2)).is_ok();
        let channel_finished = pump.finish().is_ok();
        if !stopped || !channel_finished {
            publication.preserve();
            return Err(error("STOP_UNCONFIRMED"));
        }
        return Err(error("TASK_TIMEOUT").with_cleanup(publication.cleanup()));
    }
    let mut failure_code = "TASK_FAILED";
    loop {
        let now = Instant::now();
        if cancel.load(Ordering::SeqCst) {
            let _ = task.cancel(now);
        } else {
            let _ = task.checkpoint(now);
        }
        if task.stop_reason().is_some() {
            break;
        }
        let handled = match pump.next() {
            Ok(Some(Message::Hello {})) => {
                match launcher
                    .recheck(deadline, &cancel)
                    .and_then(|_| planned.tools.recheck(&mut || checkpoint(deadline, &cancel)))
                    .and_then(|_| planned.sources.recheck(deadline, &cancel))
                {
                    Err(err) if matches!(err.code, "TASK_TIMEOUT" | "TASK_CANCELLED") => {
                        if cancel.load(Ordering::SeqCst) {
                            let _ = task.cancel(Instant::now());
                        } else {
                            let _ = task.checkpoint(Instant::now());
                        }
                        Ok(())
                    }
                    Err(err) => {
                        failure_code = err.code;
                        task.preparation_failed(Instant::now()).map_err(|_| ())
                    }
                    Ok(()) => task
                        .hello(Instant::now())
                        .and_then(|_| task.start(Instant::now()))
                        .map_err(|_| ())
                        .and_then(|_| group.start(planned.digest).map_err(|_| ())),
                }
            }
            Ok(Some(Message::Progress { stage })) => task
                .progress(Instant::now(), stage)
                .map(|_| notice(Notice::Stage(stage)))
                .map_err(|_| ()),
            Ok(Some(Message::Ready {
                plan_digest,
                artifact,
            })) => task
                .ready(Instant::now(), plan_digest, artifact)
                .map_err(|_| ()),
            Ok(Some(Message::Failed { code })) => {
                failure_code = code.as_str();
                task.worker_failed(Instant::now(), code).map_err(|_| ())
            }
            Ok(Some(Message::Rejected { code })) => {
                failure_code = rejection(code).code;
                task.worker_failed(
                    Instant::now(),
                    shield_core::diagnostic::FailureCode::Unknown,
                )
                .map_err(|_| ())
            }
            Ok(None) => {
                if group.leader_exited().unwrap_or(true) {
                    let _ = task.channel_closed(Instant::now());
                }
                Ok(())
            }
            Err(_) => {
                let _ = task.channel_closed(Instant::now());
                Err(())
            }
            _ => Err(()),
        };
        if handled.is_err() {
            failure_code = "PROTOCOL_FAILED";
            if task.stop_reason().is_none() {
                let _ = task.protocol_failed(Instant::now());
            }
        }
    }
    if group.stop(Duration::from_secs(2)).is_err() {
        let _ = task.stop_unconfirmed(Instant::now());
        publication.preserve();
        return Err(error("STOP_UNCONFIRMED"));
    }
    match pump.finish() {
        Err(()) => {
            publication.preserve();
            return Err(error("STOP_UNCONFIRMED"));
        }
        Ok(false) => {
            let _ = task.protocol_failed(Instant::now());
            failure_code = "PROTOCOL_FAILED";
        }
        Ok(true) => {}
    }
    task.tree_stopped(Instant::now())
        .map_err(|_| error("STOP_UNCONFIRMED"))?;
    let mut artifact = None;
    if task.phase() == Phase::Finalizing {
        artifact = task.artifact();
        let verification = launcher
            .verify(deadline, &cancel)
            .and_then(|_| planned.tools.recheck(&mut || checkpoint(deadline, &cancel)))
            .and_then(|_| verify(&root, &planned.sources, deadline, &cancel));
        if let Err(err) = &verification {
            if matches!(
                err.code,
                "EXECUTOR_CHANGED" | "TOOLCHAIN_CHANGED" | "PLAN_SOURCE_CHANGED"
            ) {
                failure_code = err.code;
            }
        }
        let valid = verification.is_ok();
        if cancel.load(Ordering::SeqCst) {
            let _ = task.cancel(Instant::now());
        }
        let _ = task.verified(Instant::now(), valid);
    }
    if task.phase() == Phase::ReadyToPublish {
        notice(Notice::Publishing);
        let evidence = artifact.ok_or_else(|| error("ARTIFACT_INVALID"))?;
        let receipt = publication.publish_after_stop(evidence.sha256, |point| {
            publication_checkpoint(&mut task, point, deadline, &cancel, || {
                launcher
                    .recheck(deadline, &cancel)
                    .and_then(|_| planned.tools.recheck(&mut || checkpoint(deadline, &cancel)))
                    .and_then(|_| {
                        let checked = planned.sources.recheck(deadline, &cancel);
                        if checked
                            .as_ref()
                            .is_err_and(|err| err.code == "PLAN_SOURCE_CHANGED")
                        {
                            failure_code = "PLAN_SOURCE_CHANGED";
                        }
                        checked
                    })
            })
        });
        match receipt {
            Ok(receipt) => {
                task.committed(
                    receipt.location_confirmed
                        && receipt.bytes == evidence.bytes
                        && receipt.sha256 == evidence.sha256,
                    receipt.directory_synced,
                    receipt.cleanup_complete,
                )
                .map_err(|_| error("COMMIT_UNCONFIRMED"))?;
            }
            Err(PublicationError::CommitUnconfirmed) => {
                let _ = task.commit_unconfirmed();
            }
            Err(_) if task.phase() != Phase::Finished => {
                let _ = task.publication_failed(Instant::now());
            }
            _ => {}
        }
    }
    let outcome = task
        .take_outcome()
        .ok_or_else(|| error("TASK_STATE_UNKNOWN"))?;
    let cleanup = if task.can_cleanup() {
        publication.cleanup()
    } else {
        publication.preserve();
        false
    };
    let cleanup = cleanup && planned.cleanup_complete;
    let status = match outcome.status {
        Status::Succeeded => "succeeded",
        Status::Cancelled => "cancelled",
        Status::TimedOut => "timed_out",
        Status::Unknown => "unknown",
        Status::Failed(_) => "failed",
    };
    let output = match outcome.output {
        Output::NotPublished => "not_published",
        Output::Published { .. } => "published",
        Output::Uncertain => "uncertain",
    };
    let mut result = json!({ "type":"result", "status":status, "output_state":output, "cleanup_complete":cleanup, "retryable":false, "elapsed_ms":started.elapsed().as_millis(), "plan_digest":hex(&planned.digest) });
    if let Some(binding) = planned.sources.shared_binding()? {
        result["shared_binding"] = json!(binding);
    }
    if planned.sources.content().is_some() {
        result["task_kind"] = json!("sign");
        if planned.warning {
            result["signing_warning"] = json!(true);
        }
    }
    if let Output::Published {
        directory_synced, ..
    } = outcome.output
    {
        result["directory_synced"] = json!(directory_synced);
    }
    if let Status::Failed(failure) = outcome.status {
        if failure_code == "TASK_FAILED" {
            failure_code = match failure {
                crate::automation::task_lifecycle::Failure::InvalidEvidence => {
                    "ARTIFACT_EVIDENCE_INVALID"
                }
                crate::automation::task_lifecycle::Failure::Verification => {
                    "PARENT_VERIFICATION_FAILED"
                }
                crate::automation::task_lifecycle::Failure::Publication => "PUBLICATION_FAILED",
                crate::automation::task_lifecycle::Failure::Protocol => "PROTOCOL_FAILED",
                _ => "WORKER_EXITED",
            };
        }
        result["code"] = json!(failure_code);
    }
    if let Some(artifact) = artifact {
        if output == "published" {
            result["artifact"] = json!({"bytes":artifact.bytes,"sha256":hex(&artifact.sha256)});
        }
    }
    Ok(result)
}

fn publication_checkpoint(
    task: &mut Task,
    point: PublicationCheckpoint,
    deadline: Instant,
    cancel: &AtomicBool,
    verify: impl FnOnce() -> Result<(), CheckError>,
) -> Result<(), PublicationError> {
    if cancel.load(Ordering::SeqCst) {
        let _ = task.cancel(Instant::now());
        return Err(PublicationError::Cancelled);
    }
    if point == PublicationCheckpoint::BeforeCommit {
        if let Err(err) = verify() {
            // 复核本身也包含可取消的大文件读取，不能把其控制终态降成材料失败。
            return Err(match err.code {
                "TASK_CANCELLED" => {
                    let _ = task.cancel(Instant::now());
                    PublicationError::Cancelled
                }
                "TASK_TIMEOUT" => {
                    let _ = task.checkpoint(Instant::now());
                    PublicationError::TimedOut
                }
                _ => PublicationError::InvalidArtifact,
            });
        }
    }
    if cancel.load(Ordering::SeqCst) {
        let _ = task.cancel(Instant::now());
    }
    let result = match point {
        PublicationCheckpoint::Copy => task.checkpoint(Instant::now()),
        PublicationCheckpoint::BeforeCommit => task.begin_publish(Instant::now()),
    };
    result.map_err(|_| {
        if Instant::now() >= deadline {
            PublicationError::TimedOut
        } else {
            PublicationError::Cancelled
        }
    })
}

fn verify(
    root: &Path,
    sources: &sources::Sources,
    deadline: Instant,
    cancel: &AtomicBool,
) -> Result<(), CheckError> {
    sources.recheck(deadline, cancel)?;
    sources.verify_materials(&root.join("materials"), deadline, cancel)?;
    sources.verify_content(&root.join("signed.apk"), deadline, cancel)?;
    if !install_alignment(&root.join("signed.apk"))? {
        return Err(error("ARTIFACT_INVALID"));
    }
    checkpoint(deadline, cancel)
}

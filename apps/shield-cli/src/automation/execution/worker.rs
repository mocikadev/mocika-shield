//! 内部隔离工作入口：复用同一计划与核心，只写本次私有目录。
use super::*;
use crate::args::{EnvironmentPolicyArg, KeystoreTypeArg};
use crate::automation::{
    native_group::WorkerLease,
    plan::{self, Assessment, PreparedPlan},
    snapshot,
    task_lifecycle::{ArtifactEvidence, Stage},
    task_materials::{self, MaterialBundle, Role},
    worker_protocol::{FrameWriter, Message, Peer, Rejection},
};
use shield_core::diagnostic::{Diagnostic, FailureCode};
use shield_core::{
    check_apk, extract_apk_cert_fingerprint, preflight_apk, protect_apk, sign_apk_with_progress,
    EnvironmentPolicy, KeystoreType, PreflightOptions, PreflightSeverity, ProgressStep,
    ProtectOptions, SignOptions, SigningVersions,
};
use std::sync::Mutex;
use std::time::Duration;

type Writer = Arc<Mutex<FrameWriter<std::io::Stdout>>>;

enum WorkerError {
    Policy(Rejection),
    Core(FailureCode),
}
impl From<anyhow::Error> for WorkerError {
    fn from(error: anyhow::Error) -> Self {
        Self::Core(Diagnostic::from_error(&error).code)
    }
}
impl From<CheckError> for WorkerError {
    fn from(error: CheckError) -> Self {
        Self::Policy(match error.code {
            "SIGNING_V4_NOT_SUPPORTED" => Rejection::V4Unsupported,
            "CREDENTIAL_UNAVAILABLE" => Rejection::CredentialUnavailable,
            "PLAN_SOURCE_CHANGED" => Rejection::SourceChanged,
            "SHARED_BINDING_INVALID" | "SHARED_CREDENTIAL_CONFLICT" => Rejection::InvalidRequest,
            "EXECUTABLE_CHANGED" | "EXECUTOR_CHANGED" => Rejection::ExecutorChanged,
            "TOOLCHAIN_CHANGED" => Rejection::ToolchainChanged,
            _ => {
                return Self::Core(
                    serde_json::from_value(serde_json::json!(error.code))
                        .unwrap_or(FailureCode::Unknown),
                )
            }
        })
    }
}

fn send(writer: &Writer, message: Message) -> Result<(), WorkerError> {
    writer
        .lock()
        .map_err(|_| WorkerError::Core(FailureCode::Unknown))?
        .send(message)
        .map_err(|_| WorkerError::Core(FailureCode::Unknown))
}

pub(super) fn run(args: WorkerArgs) -> i32 {
    let Ok(task_id) = parse_hex::<16>(&args.task_id) else {
        return 1;
    };
    if task_materials::validate_directory(&args.root).is_err() {
        return 1;
    }
    let Ok(lease) = WorkerLease::install(task_id) else {
        return 1;
    };
    shield_core::utils::set_json_mode(true);
    let writer = Arc::new(Mutex::new(FrameWriter::new(
        std::io::stdout(),
        task_id,
        Peer::Worker,
    )));
    if send(&writer, Message::Hello {}).is_err() {
        return 1;
    }
    let result = (|| {
        let start = lease
            .wait_start(Duration::from_secs(3600))
            .map_err(|_| WorkerError::Policy(Rejection::InvalidRequest))?;
        let (bootstrap, _) = read_json::<Bootstrap>(&args.root.join("bootstrap.json"), 65536)?;
        if bootstrap.format_version
            != if bootstrap.request.shared.is_some() {
                3
            } else {
                2
            }
        {
            return Err(WorkerError::Policy(Rejection::InvalidRequest));
        }
        bootstrap.executor.worker()?;
        match (&bootstrap.mode, &bootstrap.tools) {
            (Mode::Plan, None) => {}
            (Mode::Execute, Some(tools)) => tools.recheck(&mut || Ok(()))?,
            _ => return Err(WorkerError::Policy(Rejection::InvalidRequest)),
        }
        let mut request = bootstrap.request;
        request.validate()?;
        send(
            &writer,
            Message::Progress {
                stage: Stage::Prepare,
            },
        )?;
        let assessment = match &request.shared {
            Some(shared) => crate::automation::shared_plan::prepare(
                request.plan_args(),
                request.config.as_deref(),
                shared,
            ),
            None => plan::prepare(request.plan_args(), request.config.as_deref(), true),
        }?;
        let plan = match assessment {
            Assessment::Blocked(_) => return Err(WorkerError::Policy(Rejection::PlanBlocked)),
            Assessment::Prepared(plan) => plan,
        };
        match bootstrap.mode {
            Mode::Plan => {
                let proof = PlanProof {
                    format_version: if plan.sources.shared.is_some() { 2 } else { 1 },
                    warning: plan.result.status == crate::automation::Status::Warning,
                    sources: plan.sources,
                };
                let sources_sha256 = write_json(&args.root.join("sources.json"), &proof)?;
                send(
                    &writer,
                    Message::Planned {
                        plan_digest: plan.digest,
                        certificate_sha256: parse_hex(&shield_core::normalize_fingerprint(
                            &plan.certificate,
                        ))?,
                        sources_sha256,
                    },
                )?;
            }
            Mode::Execute => {
                let tools = bootstrap
                    .tools
                    .as_ref()
                    .ok_or(WorkerError::Policy(Rejection::InvalidRequest))?;
                tools.matches(&plan.sources)?;
                tools.recheck(&mut || Ok(()))?;
                if start != plan.digest {
                    return Err(WorkerError::Policy(Rejection::PlanMismatch));
                }
                execute(plan, &args.root, &writer, tools, &bootstrap.executor)?;
            }
        }
        Ok::<_, WorkerError>(())
    })();
    if let Err(error) = result {
        let message = match error {
            WorkerError::Policy(code) => Message::Rejected { code },
            WorkerError::Core(code) => Message::Failed { code },
        };
        if send(&writer, message).is_err() {
            return 1;
        }
    }
    // 保留组长与租约直至父端确认收尾；不能成功发消息后自行退出让 PID 提前可复用。
    loop {
        std::thread::park_timeout(Duration::from_secs(60));
    }
}

fn execute(
    plan: Box<PreparedPlan>,
    root: &Path,
    writer: &Writer,
    tools: &identity::Tools,
    executor: &identity::Executable,
) -> Result<(), WorkerError> {
    let sources = &plan.sources;
    let bundle = if sources.shared.is_some() {
        MaterialBundle::create_shared(root, &sources.materials(), || Ok(()))
    } else {
        MaterialBundle::create(root, &sources.materials(), || Ok(()))
    }
    .map_err(|_| WorkerError::Policy(Rejection::MaterialInvalid))?;
    sources.recheck()?;
    MaterialBundle::verify(&root.join("materials"), bundle.digest(), || Ok(()))
        .map_err(|_| WorkerError::Policy(Rejection::MaterialInvalid))?;
    let input = bundle.path(Role::Input);
    let signer = bundle.path(Role::Apksigner);
    tools.recheck(&mut || Ok(()))?;
    let report = preflight_apk(
        &input,
        PreflightOptions {
            runtime_profile: plan.profile,
            apksigner_path: Some(&signer),
            expected_output_cert_fingerprint: Some(&plan.certificate),
        },
    )?;
    if report.verdict == PreflightSeverity::Blocked {
        return Err(WorkerError::Policy(Rejection::PlanBlocked));
    }
    let policy = match plan.protect.environment_policy {
        EnvironmentPolicyArg::Compatible => EnvironmentPolicy::Compatible,
        EnvironmentPolicyArg::Strict => EnvironmentPolicy::Strict,
    };
    crate::automation::resources::validate(
        &bundle.path(Role::Resources),
        plan.profile,
        policy,
        &report.facts.native_abis,
        &plan.protect.excluded_abis,
    )?;
    let unsigned = root.join("protected-unsigned.apk");
    let signed = root.join("signed.apk");
    let options = ProtectOptions {
        input,
        output: unsigned.clone(),
        excluded_abis: plan.protect.excluded_abis,
        apktool_path: Some(bundle.path(Role::Apktool)),
        resources_path: Some(bundle.path(Role::Resources)),
        apksigner_path: Some(signer.clone()),
        expected_output_cert_fingerprint: Some(plan.certificate.clone()),
        environment_policy: policy,
    };
    let cancelled = Arc::new(AtomicBool::new(false));
    let cancelled_progress = cancelled.clone();
    let progress_writer = writer.clone();
    let last_stage = Mutex::new(None::<Stage>);
    let tool_changed = Arc::new(AtomicBool::new(false));
    let changed_progress = tool_changed.clone();
    let progress_tools = tools.clone();
    let protected = protect_apk(
        &options,
        move |event| {
            if progress_tools.recheck(&mut || Ok(())).is_err() {
                changed_progress.store(true, Ordering::SeqCst);
                cancelled_progress.store(true, Ordering::SeqCst);
                return;
            }
            let stage = match event.step {
                ProgressStep::CheckTools => Stage::CheckTools,
                ProgressStep::Unpack => Stage::Unpack,
                ProgressStep::ModifyManifest => Stage::ModifyManifest,
                ProgressStep::ProcessDex => Stage::ProcessDex,
                ProgressStep::InjectRuntime => Stage::InjectRuntime,
                ProgressStep::Repack => Stage::Repack,
                ProgressStep::AlignApk => Stage::Align,
            };
            let Ok(mut last) = last_stage.lock() else {
                cancelled_progress.store(true, Ordering::SeqCst);
                return;
            };
            if *last == Some(stage) {
                return;
            }
            if last.is_some_and(|previous| stage < previous)
                || send(&progress_writer, Message::Progress { stage }).is_err()
            {
                cancelled_progress.store(true, Ordering::SeqCst);
            }
            *last = Some(stage);
        },
        cancelled,
    );
    if tool_changed.load(Ordering::SeqCst) {
        return Err(WorkerError::Policy(Rejection::ToolchainChanged));
    }
    protected.map_err(|error| {
        WorkerError::Core(
            Diagnostic::from_shield(&error)
                .map(|diagnostic| diagnostic.code)
                .unwrap_or(FailureCode::Unknown),
        )
    })?;
    executor.worker()?;
    tools.recheck(&mut || Ok(()))?;
    send(writer, Message::Progress { stage: Stage::Sign })?;
    let options = SignOptions {
        apk_path: unsigned,
        output_path: Some(signed.clone()),
        keystore_path: bundle.path(Role::Keystore),
        key_alias: plan.sign.key_alias,
        keystore_password: plan.sign.keystore_password,
        key_password: plan.sign.key_password,
        apksigner_path: Some(signer.clone()),
        keystore_type: match plan.sign.keystore_type {
            KeystoreTypeArg::Jks => KeystoreType::Jks,
            KeystoreTypeArg::Pkcs12 => KeystoreType::Pkcs12,
        },
        signing_versions: SigningVersions {
            v1: plan.sign.v1,
            v2: plan.sign.v2,
            v3: plan.sign.v3,
            v4: false,
        },
    };
    sign_apk_with_progress(&options, |_| Ok(()))?;
    tools.recheck(&mut || Ok(()))?;
    send(
        writer,
        Message::Progress {
            stage: Stage::Verify,
        },
    )?;
    let checked = check_apk(&signed, Some(&signer))?;
    let certificate = extract_apk_cert_fingerprint(&signed, Some(&signer))?;
    if !checked.already_protected {
        return Err(WorkerError::Policy(Rejection::ProtectionMissing));
    }
    if !checked.is_signed {
        return Err(WorkerError::Policy(Rejection::SignatureInvalid));
    }
    if shield_core::normalize_fingerprint(&certificate)
        != shield_core::normalize_fingerprint(&plan.certificate)
    {
        return Err(WorkerError::Policy(Rejection::CertificateMismatch));
    }
    if !install_alignment(&signed)? {
        return Err(WorkerError::Policy(Rejection::AlignmentInvalid));
    }
    sources.recheck()?;
    tools.recheck(&mut || Ok(()))?;
    executor.worker()?;
    let artifact = snapshot::file("artifact", &signed)?;
    send(
        writer,
        Message::Ready {
            plan_digest: plan.digest,
            artifact: ArtifactEvidence {
                bytes: artifact.size,
                sha256: parse_hex(&artifact.sha256)?,
                certificate_sha256: parse_hex(&shield_core::normalize_fingerprint(&certificate))?,
                signature_checked: true,
                protection_checked: true,
                alignment_checked: true,
            },
        },
    )
}

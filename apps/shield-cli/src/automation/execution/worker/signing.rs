//! 仅签名工作分支：材料私有固化、实际签名与内容验收；只写本次暂存，不负责最终交付。
use super::*;
use crate::automation::sign_plan;

pub(super) fn run(
    request: &Request,
    bootstrap: &Bootstrap,
    expected: [u8; 32],
    root: &Path,
    writer: &Writer,
) -> Result<(), WorkerError> {
    let sign_request = request
        .sign
        .as_ref()
        .ok_or(WorkerError::Policy(Rejection::InvalidRequest))?;
    let plan = sign_plan::prepare(
        &request.input,
        &request.output,
        request.config.as_deref(),
        sign_request,
        root,
    )?;
    match bootstrap.mode {
        Mode::Plan => {
            let proof = PlanProof {
                format_version: request.proof_version(),
                warning: plan.warning,
                sources: sources::Sources::Sign(Box::new(plan.sources)),
            };
            let sources_sha256 = write_json(&root.join("sources.json"), &proof)?;
            send(
                writer,
                Message::Planned {
                    plan_digest: plan.digest,
                    certificate_sha256: plan.certificate,
                    sources_sha256,
                },
            )
        }
        Mode::Execute => {
            if expected != plan.digest {
                return Err(WorkerError::Policy(Rejection::PlanMismatch));
            }
            execute(
                plan,
                root,
                writer,
                bootstrap
                    .tools
                    .as_ref()
                    .ok_or(WorkerError::Policy(Rejection::InvalidRequest))?,
                &bootstrap.executor,
            )
        }
    }
}

fn execute(
    plan: sign_plan::Prepared,
    root: &Path,
    writer: &Writer,
    tools: &identity::Tools,
    executor: &identity::Executable,
) -> Result<(), WorkerError> {
    let sources = &plan.sources;
    tools.matches_files(&sources.java, &sources.keytool)?;
    tools.recheck(&mut || Ok(()))?;
    let bundle = MaterialBundle::create_version(
        root,
        &sources.materials(),
        sources.material_version(),
        || Ok(()),
    )
    .map_err(|_| WorkerError::Policy(Rejection::MaterialInvalid))?;
    snapshot::recheck(&sources.files())?;
    sources.recheck_record()?;
    MaterialBundle::verify_sources_version(
        &root.join("materials"),
        &sources.materials(),
        sources.material_version(),
        || Ok(()),
    )
    .map_err(|_| WorkerError::Policy(Rejection::MaterialInvalid))?;
    let input = bundle.path(Role::Input);
    let content = shield_core::inspect_signing_input(&input, || Ok(()))?;
    if content.content_sha256 != sources.content_sha256
        || content.certificate_binding != sources.certificate_binding
    {
        return Err(WorkerError::Policy(Rejection::SourceChanged));
    }
    let signed = root.join("signed.apk");
    let signer = bundle.path(Role::Apksigner);
    let bound = sources
        .java_adapter
        .as_ref()
        .map(|binding| {
            crate::automation::adapter::Bound::new(binding, &bundle.path(Role::JavaAdapter), root)
        })
        .transpose()?;
    let java_tools = crate::automation::java_tools::Tools::new(
        &sources.java.path,
        bound.as_ref().map(|b| &b.context),
    );
    let options = SignOptions {
        apk_path: input,
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
    java_tools.sign(&options, |step| {
        executor.worker().map_err(|_| "执行器发生变化".to_owned())?;
        tools
            .recheck(&mut || Ok(()))
            .map_err(|_| "签名工具发生变化".to_owned())?;
        if let Some(bound) = &bound {
            bound.recheck().map_err(|_| "适配材料发生变化".to_owned())?;
        }
        let stage = match step {
            shield_core::SigningProgressStep::Prepare => Stage::CheckTools,
            shield_core::SigningProgressStep::Align => Stage::Align,
            shield_core::SigningProgressStep::Sign => Stage::Sign,
        };
        send(writer, Message::Progress { stage }).map_err(|_| "签名进度通道不可用".to_owned())
    })?;
    send(
        writer,
        Message::Progress {
            stage: Stage::Verify,
        },
    )?;
    tools.recheck(&mut || Ok(()))?;
    if let Some(bound) = &bound {
        bound.recheck()?;
    }
    let checked = java_tools.inspect(&signed, &signer)?;
    if !checked.is_signed {
        return Err(WorkerError::Policy(Rejection::SignatureInvalid));
    }
    let certificate = java_tools.fingerprint(&signed, &signer)?;
    if parse_hex::<32>(&shield_core::normalize_fingerprint(&certificate))? != plan.certificate {
        return Err(WorkerError::Policy(Rejection::CertificateMismatch));
    }
    if !install_alignment(&signed)?
        || root
            .join("signed.apk.idsig")
            .try_exists()
            .map_err(anyhow::Error::from)?
    {
        return Err(WorkerError::Policy(Rejection::AlignmentInvalid));
    }
    let actual = shield_core::inspect_signing_input(&signed, || Ok(()))?;
    if actual != content {
        return Err(WorkerError::Policy(Rejection::ValidationFailed));
    }
    snapshot::recheck(&sources.files())?;
    if let Some(bound) = &bound {
        bound.recheck()?;
    }
    sources.recheck_record()?;
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
                certificate_sha256: plan.certificate,
                signature_checked: true,
                protection_checked: false,
                alignment_checked: true,
                content_sha256: Some(actual.content_sha256),
            },
        },
    )
}

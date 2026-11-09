use crate::app_config::normalize_keystore_type;
use crate::app_paths::find_apksigner_path;
use crate::cert_store::CertificateRecord;
use crate::failure_diagnostic::ExecutionFailure;
use shield_core::diagnostic::{Diagnostic, ToolName};
use shield_core::{
    sign_apk_with_progress as shield_sign_apk, KeystoreType, SignOptions, SigningProgressStep,
    SigningVersions,
};
use std::path::PathBuf;

pub(crate) fn execute_sign_apk(
    app: &tauri::AppHandle,
    apk_path: String,
    output_path: Option<String>,
    apksigner_path: Option<String>,
    certificate: CertificateRecord,
    mut on_progress: impl FnMut(&str, &str) -> Result<(), String>,
) -> Result<(), ExecutionFailure> {
    let resolved_apksigner = apksigner_path
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .or_else(|| find_apksigner_path(app));
    let ks_pass = certificate.keystore_password;
    let key_pass = certificate.key_password;
    let effective_key_pass = if key_pass.is_empty() {
        ks_pass.clone()
    } else {
        key_pass
    };
    let opts = SignOptions {
        apk_path: PathBuf::from(apk_path),
        output_path: output_path.filter(|s| !s.is_empty()).map(PathBuf::from),
        keystore_path: PathBuf::from(certificate.keystore_path),
        key_alias: certificate.key_alias,
        keystore_password: ks_pass,
        key_password: effective_key_pass,
        apksigner_path: resolved_apksigner,
        keystore_type: KeystoreType::parse(
            normalize_keystore_type(Some(certificate.ks_type.as_str()))
                .as_deref()
                .unwrap_or("JKS"),
        ),
        signing_versions: SigningVersions {
            v1: certificate.sign_v1,
            v2: certificate.sign_v2,
            v3: certificate.sign_v3,
            v4: certificate.sign_v4,
        },
    };
    shield_sign_apk(&opts, |step| {
        let (step, message) = match step {
            SigningProgressStep::Prepare => ("PrepareSign", "准备签名参数"),
            SigningProgressStep::Align => ("AlignApk", "对齐待签名 APK"),
            SigningProgressStep::Sign => ("SignApk", "调用 apksigner 执行签名"),
        };
        on_progress(step, message)
    })
    .map_err(|e| {
        let mut failure = ExecutionFailure::diagnosed(e.to_string(), Diagnostic::from_error(&e));
        if failure.diagnostic.tool == Some(ToolName::Apksigner) {
            failure.tool_path = opts.apksigner_path.clone();
        }
        failure
    })?;
    Ok(())
}

pub(crate) fn query_keystore_aliases(
    keystore_path: String,
    ks_pass: String,
    ks_type: Option<String>,
) -> Result<Vec<String>, ExecutionFailure> {
    shield_services::certificates::query_keystore_aliases(keystore_path, ks_pass, ks_type)
        .map_err(Into::into)
}

use crate::cert_store::CertificateRecord;
use serde::Serialize;
use shield_core::{
    extract_apk_cert_fingerprint, extract_keystore_cert_fingerprint, normalize_fingerprint,
    preflight_apk, PreflightCheck, PreflightReport, PreflightSeverity, RuntimeProfile,
};
use std::path::{Path, PathBuf};

#[derive(Debug, Serialize)]
pub(crate) struct ApkCheckResult {
    pub verdict: &'static str,
    pub checks: Vec<ApkPreflightCheck>,
    pub facts: ApkPreflightFacts,
    pub error_code: Option<&'static str>,
    pub error: Option<String>,
}

#[derive(Debug, Serialize)]
pub(crate) struct ApkPreflightCheck {
    pub code: &'static str,
    pub severity: &'static str,
    pub detail: Option<String>,
}

#[derive(Debug, Serialize, Default)]
pub(crate) struct ApkPreflightFacts {
    pub apk_size: u64,
    pub dex_count: u32,
    pub dex_total_size: u64,
    pub native_library_count: u32,
    pub compressed_native_library_count: u32,
    pub native_abis: Vec<String>,
}

#[derive(Debug, Serialize)]
pub(crate) struct CertCompareResult {
    pub matches: bool,
    pub apk_fingerprint: Option<String>,
    pub ks_fingerprint: Option<String>,
    pub error: Option<String>,
}

pub(crate) fn do_compare_cert_fingerprints(
    apk_path: String,
    keystore_path: String,
    ks_pass: String,
    ks_type: Option<String>,
    key_alias: String,
    apksigner_path: Option<PathBuf>,
) -> CertCompareResult {
    let apk_fp = extract_apk_fingerprint(&apk_path, apksigner_path.as_deref());
    let ks_fp =
        extract_keystore_fingerprint(&keystore_path, &ks_pass, ks_type.as_deref(), &key_alias);

    match (apk_fp, ks_fp) {
        (Ok(apk), Ok(ks)) => {
            let matches = normalize_fingerprint(&apk) == normalize_fingerprint(&ks);
            CertCompareResult {
                matches,
                apk_fingerprint: Some(apk),
                ks_fingerprint: Some(ks),
                error: None,
            }
        }
        (Err(e), _) => CertCompareResult {
            matches: false,
            apk_fingerprint: None,
            ks_fingerprint: None,
            error: Some(format!("读取 APK 签名失败: {e}")),
        },
        (_, Err(e)) => CertCompareResult {
            matches: false,
            apk_fingerprint: None,
            ks_fingerprint: None,
            error: Some(format!("读取 keystore 失败: {e}")),
        },
    }
}

fn extract_apk_fingerprint(
    apk_path: &str,
    apksigner_path: Option<&Path>,
) -> Result<String, String> {
    extract_apk_cert_fingerprint(Path::new(apk_path), apksigner_path).map_err(|e| e.to_string())
}

fn extract_keystore_fingerprint(
    keystore_path: &str,
    ks_pass: &str,
    ks_type: Option<&str>,
    key_alias: &str,
) -> Result<String, String> {
    extract_keystore_cert_fingerprint(Path::new(keystore_path), key_alias, ks_pass, ks_type)
        .map_err(|e| e.to_string())
}

pub(crate) fn do_check_apk(
    path: String,
    apksigner_path: Option<PathBuf>,
    runtime_profile: RuntimeProfile,
    certificate: Option<CertificateRecord>,
) -> ApkCheckResult {
    let expected_output_cert_fingerprint = certificate
        .as_ref()
        .map(|certificate| {
            extract_keystore_cert_fingerprint(
                Path::new(&certificate.keystore_path),
                &certificate.key_alias,
                &certificate.keystore_password,
                Some(&certificate.ks_type),
            )
        })
        .transpose();
    let expected_output_cert_fingerprint = match expected_output_cert_fingerprint {
        Ok(value) => value,
        Err(error) => {
            return ApkCheckResult {
                verdict: "blocked",
                checks: Vec::new(),
                facts: ApkPreflightFacts::default(),
                error_code: Some("certificate_unreadable"),
                error: Some(format!("读取所选签名证书失败：{error}")),
            };
        }
    };
    match preflight_apk(
        Path::new(&path),
        shield_core::PreflightOptions {
            runtime_profile,
            apksigner_path: apksigner_path.as_deref(),
            expected_output_cert_fingerprint: expected_output_cert_fingerprint.as_deref(),
        },
    ) {
        Ok(report) => map_report(report),
        Err(error) => ApkCheckResult {
            verdict: "blocked",
            checks: Vec::new(),
            facts: ApkPreflightFacts::default(),
            error_code: Some("inspection_failed"),
            error: Some(error.to_string()),
        },
    }
}

fn map_report(report: PreflightReport) -> ApkCheckResult {
    ApkCheckResult {
        verdict: severity_value(report.verdict),
        checks: report.checks.into_iter().map(map_check).collect(),
        facts: ApkPreflightFacts {
            apk_size: report.facts.apk_size,
            dex_count: report.facts.dex_count,
            dex_total_size: report.facts.dex_total_size,
            native_library_count: report.facts.native_library_count,
            compressed_native_library_count: report.facts.compressed_native_library_count,
            native_abis: report.facts.native_abis,
        },
        error_code: None,
        error: None,
    }
}

fn map_check(check: PreflightCheck) -> ApkPreflightCheck {
    ApkPreflightCheck {
        code: check.code,
        severity: severity_value(check.severity),
        detail: check.detail,
    }
}

fn severity_value(severity: PreflightSeverity) -> &'static str {
    match severity {
        PreflightSeverity::Ready => "ready",
        PreflightSeverity::Warning => "warning",
        PreflightSeverity::Blocked => "blocked",
    }
}

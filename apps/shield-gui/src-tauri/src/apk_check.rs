use serde::Serialize;
use shield_core::{
    check_apk, extract_apk_cert_fingerprint, extract_keystore_cert_fingerprint,
    normalize_fingerprint,
};
use std::path::{Path, PathBuf};

#[derive(Debug, Serialize)]
pub(crate) struct ApkCheckResult {
    pub already_protected: bool,
    pub is_signed: bool,
    pub error: Option<String>,
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

pub(crate) fn do_check_apk(path: String, apksigner_path: Option<PathBuf>) -> ApkCheckResult {
    match check_apk(Path::new(&path), apksigner_path.as_deref()) {
        Ok(result) => ApkCheckResult {
            already_protected: result.already_protected,
            is_signed: result.is_signed,
            error: None,
        },
        Err(error) => ApkCheckResult {
            already_protected: false,
            is_signed: false,
            error: Some(error.to_string()),
        },
    }
}

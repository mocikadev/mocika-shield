use serde::Serialize;
use shield_cli::utils::no_window_command;
use std::fs;
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

fn extract_apk_fingerprint(apk_path: &str, apksigner_path: Option<&Path>) -> Result<String, String> {
    if let Ok(output) = no_window_command("keytool")
        .args(["-printcert", "-jarfile", apk_path])
        .output()
    {
        if output.status.success() {
            let text = String::from_utf8_lossy(&output.stdout);
            if let Some(fp) = parse_sha256_fingerprint(&text) {
                return Ok(fp);
            }
        }
    }

    let signer =
        apksigner_path.ok_or_else(|| "V1 证书提取失败，且未找到 apksigner.jar".to_string())?;
    let result = no_window_command("java")
        .args([
            "-jar",
            signer.to_str().unwrap_or(""),
            "verify",
            "--print-certs",
            apk_path,
        ])
        .output()
        .map_err(|e| format!("执行 apksigner verify 失败: {e}"))?;

    if result.status.success() {
        let stdout = String::from_utf8_lossy(&result.stdout);
        if let Some(fp) = parse_sha256_from_apksigner(&stdout) {
            return Ok(fp);
        }
    }

    Err("无法提取 APK 证书指纹（V1 和 V2/V3 均失败）".to_string())
}

fn extract_keystore_fingerprint(
    keystore_path: &str,
    ks_pass: &str,
    ks_type: Option<&str>,
    key_alias: &str,
) -> Result<String, String> {
    let mut args = vec![
        "-list",
        "-v",
        "-keystore",
        keystore_path,
        "-alias",
        key_alias,
        "-storepass",
        ks_pass,
    ];
    if let Some(t) = ks_type {
        args.push("-storetype");
        args.push(t);
    }

    let output = no_window_command("keytool")
        .args(&args)
        .output()
        .map_err(|e| format!("启动 keytool 失败: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("keytool 执行失败: {}", stderr.trim()));
    }

    let text = String::from_utf8_lossy(&output.stdout);
    parse_sha256_fingerprint(&text).ok_or_else(|| "未能从 keystore 中解析 SHA256 指纹".to_string())
}

fn parse_sha256_fingerprint(text: &str) -> Option<String> {
    for line in text.lines() {
        let trimmed = line.trim();
        if let Some(pos) = trimmed.to_uppercase().find("SHA256") {
            let after = &trimmed[pos..];
            if let Some(colon_pos) = after.find(':') {
                let fp = after[colon_pos + 1..].trim();
                if fp.contains(':') && fp.len() > 30 {
                    return Some(fp.to_string());
                }
            }
        }
    }
    None
}

fn parse_sha256_from_apksigner(output: &str) -> Option<String> {
    for line in output.lines() {
        let upper = line.trim().to_uppercase();
        if upper.contains("SHA-256") && upper.contains("DIGEST") {
            if let Some(pos) = line.rfind(':') {
                let hex = line[pos + 1..].trim().to_uppercase();
                if hex.len() == 64 && hex.chars().all(|c| c.is_ascii_hexdigit()) {
                    return Some(hex);
                }
            }
        }
    }
    None
}

fn normalize_fingerprint(fp: &str) -> String {
    fp.to_uppercase().replace(':', "").replace(' ', "")
}

pub(crate) fn do_check_apk(path: String, apksigner_path: Option<PathBuf>) -> ApkCheckResult {
    let apk_path = PathBuf::from(&path);

    let file = match fs::File::open(&apk_path) {
        Ok(f) => f,
        Err(e) => {
            return ApkCheckResult {
                already_protected: false,
                is_signed: false,
                error: Some(format!("无法打开 APK 文件: {e}")),
            }
        }
    };

    let mut archive = match zip::ZipArchive::new(file) {
        Ok(a) => a,
        Err(e) => {
            return ApkCheckResult {
                already_protected: false,
                is_signed: false,
                error: Some(format!("无法解析 APK（ZIP 格式错误）: {e}")),
            }
        }
    };

    let mut already_protected = false;
    let mut classes_dex_error: Option<String> = None;

    const MSHD_MAGIC: &[u8] = b"MSHD";
    const TAIL_READ_SIZE: u64 = 4096;

    for i in 0..archive.len() {
        let mut entry = match archive.by_index(i) {
            Ok(e) => e,
            Err(_) => continue,
        };
        let name = entry.name().to_owned();

        if name == "classes.dex" && !already_protected {
            use std::io::Read;
            let entry_size = entry.size();
            if entry_size >= 8 {
                let skip = entry_size.saturating_sub(TAIL_READ_SIZE);
                let read_len = (entry_size - skip) as usize;
                let mut tail = vec![0u8; read_len];
                let skip_result = if skip > 0 {
                    std::io::copy(&mut entry.by_ref().take(skip), &mut std::io::sink())
                        .map(|_| ())
                        .map_err(|e| e.to_string())
                } else {
                    Ok(())
                };
                match skip_result
                    .and_then(|_| entry.read_exact(&mut tail).map_err(|e| e.to_string()))
                {
                    Ok(()) => {
                        already_protected = tail.windows(MSHD_MAGIC.len()).any(|w| w == MSHD_MAGIC);
                    }
                    Err(e) => {
                        classes_dex_error = Some(format!("classes.dex 读取失败: {e}"));
                    }
                }
            }
        }

        if already_protected {
            break;
        }
    }

    if let Some(err) = classes_dex_error {
        return ApkCheckResult {
            already_protected: false,
            is_signed: false,
            error: Some(err),
        };
    }

    let is_signed = check_apk_signed(&apk_path, apksigner_path.as_deref());

    ApkCheckResult {
        already_protected,
        is_signed,
        error: None,
    }
}

fn check_apk_signed(apk_path: &PathBuf, apksigner_path: Option<&Path>) -> bool {
    if let Some(signer) = apksigner_path {
        if let Ok(status) = no_window_command("java")
            .args([
                "-jar",
                signer.to_str().unwrap_or(""),
                "verify",
                apk_path.to_str().unwrap_or(""),
            ])
            .status()
        {
            return status.success();
        }
    }

    let Ok(file) = fs::File::open(apk_path) else {
        return false;
    };
    let Ok(mut archive) = zip::ZipArchive::new(file) else {
        return false;
    };
    for i in 0..archive.len() {
        let Ok(entry) = archive.by_index(i) else {
            continue;
        };
        let name = entry.name();
        if name.starts_with("META-INF/")
            && (name.ends_with(".RSA") || name.ends_with(".DSA") || name.ends_with(".EC"))
        {
            return true;
        }
    }
    false
}

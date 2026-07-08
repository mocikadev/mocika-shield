use anyhow::{Context, Result};
use std::fs;
use std::io::Read;
use std::path::Path;

use crate::utils::{find_apksigner, find_java, find_keytool, no_window_command};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApkCheckOutcome {
    pub already_protected: bool,
    pub is_signed: bool,
}

pub fn check_apk(path: &Path, apksigner_path: Option<&Path>) -> Result<ApkCheckOutcome> {
    let file =
        fs::File::open(path).with_context(|| format!("无法打开 APK 文件: {}", path.display()))?;
    let mut archive = zip::ZipArchive::new(file).context("无法解析 APK（ZIP 格式错误）")?;

    let mut already_protected = false;
    const MSHD_MAGIC: &[u8] = b"MSHD";
    const TAIL_READ_SIZE: u64 = 4096;

    for i in 0..archive.len() {
        let mut entry = match archive.by_index(i) {
            Ok(entry) => entry,
            Err(_) => continue,
        };

        if entry.name() == "classes.dex" && !already_protected {
            let entry_size = entry.size();
            if entry_size >= 8 {
                let skip = entry_size.saturating_sub(TAIL_READ_SIZE);
                let read_len = (entry_size - skip) as usize;
                let mut tail = vec![0u8; read_len];
                if skip > 0 {
                    std::io::copy(&mut entry.by_ref().take(skip), &mut std::io::sink())
                        .context("读取 classes.dex 失败")?;
                }
                entry
                    .read_exact(&mut tail)
                    .context("读取 classes.dex 失败")?;
                already_protected = tail.windows(MSHD_MAGIC.len()).any(|w| w == MSHD_MAGIC);
            }
        }

        if already_protected {
            break;
        }
    }

    Ok(ApkCheckOutcome {
        already_protected,
        is_signed: check_apk_signed(path, apksigner_path)?,
    })
}

pub fn extract_apk_cert_fingerprint(
    apk_path: &Path,
    apksigner_path: Option<&Path>,
) -> Result<String> {
    if let Ok(keytool) = find_keytool() {
        if let Ok(output) = no_window_command(&keytool)
            .args(["-printcert", "-jarfile", apk_path.to_str().unwrap_or("")])
            .output()
        {
            if output.status.success() {
                let stdout = String::from_utf8_lossy(&output.stdout);
                if let Some(fp) = parse_sha256_from_keytool(&stdout) {
                    return Ok(fp);
                }
            }
        }
    }

    let signer = match apksigner_path {
        Some(path) => path.to_path_buf(),
        None => find_apksigner().context("V1 证书提取失败，且未找到 apksigner.jar")?,
    };
    let java = find_java().context("V1 证书提取失败，且未找到 Java")?;
    let output = no_window_command(&java)
        .args([
            "-jar",
            signer.to_str().unwrap_or(""),
            "verify",
            "--print-certs",
            apk_path.to_str().unwrap_or(""),
        ])
        .output()
        .context("执行 apksigner verify 失败")?;

    if output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        if let Some(fp) = parse_sha256_from_apksigner(&stdout) {
            return Ok(fp);
        }
    }

    anyhow::bail!("无法提取 APK 证书指纹（V1 和 V2/V3 均失败）")
}

pub fn extract_keystore_cert_fingerprint(
    ks: &Path,
    alias: &str,
    ks_pass: &str,
    ks_type: Option<&str>,
) -> Result<String> {
    let keytool = find_keytool()?;

    let mut args = vec![
        "-list",
        "-v",
        "-keystore",
        ks.to_str().unwrap_or(""),
        "-alias",
        alias,
        "-storepass",
        ks_pass,
    ];
    if let Some(kind) = ks_type {
        args.push("-storetype");
        args.push(kind);
    }

    let output = no_window_command(&keytool)
        .args(&args)
        .output()
        .context("执行 keytool 失败")?;

    if output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        if let Some(fp) = parse_sha256_from_keytool(&stdout) {
            return Ok(fp);
        }
    }

    let stderr = String::from_utf8_lossy(&output.stderr);
    anyhow::bail!("无法读取 keystore 证书指纹：{}", stderr.trim())
}

pub fn normalize_fingerprint(fp: &str) -> String {
    fp.to_uppercase().replace(':', "").replace(' ', "")
}

fn check_apk_signed(apk_path: &Path, apksigner_path: Option<&Path>) -> Result<bool> {
    if let Some(signer) = apksigner_path {
        let java = find_java()?;
        if let Ok(status) = no_window_command(&java)
            .args([
                "-jar",
                signer.to_str().unwrap_or(""),
                "verify",
                apk_path.to_str().unwrap_or(""),
            ])
            .status()
        {
            return Ok(status.success());
        }
    }

    if has_v2_v3_signature(apk_path) {
        return Ok(true);
    }

    let Ok(file) = fs::File::open(apk_path) else {
        return Ok(false);
    };
    let Ok(mut archive) = zip::ZipArchive::new(file) else {
        return Ok(false);
    };
    for i in 0..archive.len() {
        let Ok(entry) = archive.by_index(i) else {
            continue;
        };
        let name = entry.name();
        if name.starts_with("META-INF/")
            && (name.ends_with(".RSA") || name.ends_with(".DSA") || name.ends_with(".EC"))
        {
            return Ok(true);
        }
    }
    Ok(false)
}

fn has_v2_v3_signature(apk_path: &Path) -> bool {
    use std::io::{Read, Seek, SeekFrom};

    const MAGIC: &[u8] = b"APK Sig Block 42";
    let Ok(mut file) = fs::File::open(apk_path) else {
        return false;
    };
    let Ok(meta) = file.metadata() else {
        return false;
    };
    let file_size = meta.len();
    let scan_size = file_size.min(65536) as usize;
    let offset = file_size.saturating_sub(scan_size as u64);
    if file.seek(SeekFrom::Start(offset)).is_err() {
        return false;
    }
    let mut buffer = vec![0u8; scan_size];
    let read = file.read(&mut buffer).unwrap_or(0);
    buffer.truncate(read);
    buffer.windows(MAGIC.len()).any(|w| w == MAGIC)
}

fn parse_sha256_from_keytool(output: &str) -> Option<String> {
    for line in output.lines() {
        let trimmed = line.trim();
        if trimmed.to_uppercase().contains("SHA256") && trimmed.contains(':') {
            let after_label = trimmed.split_once(':')?.1;
            let clean = after_label.replace(':', "").replace(' ', "").to_uppercase();
            if clean.len() == 64 && clean.chars().all(|c| c.is_ascii_hexdigit()) {
                return Some(clean);
            }
        }
    }
    None
}

fn parse_sha256_from_apksigner(output: &str) -> Option<String> {
    for line in output.lines() {
        let upper = line.trim().to_uppercase();
        if upper.contains("SHA-256") && upper.contains("DIGEST") {
            let pos = line.rfind(':')?;
            let hex = line[pos + 1..].trim().to_uppercase();
            if hex.len() == 64 && hex.chars().all(|c| c.is_ascii_hexdigit()) {
                return Some(hex);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::{normalize_fingerprint, parse_sha256_from_apksigner, parse_sha256_from_keytool};

    #[test]
    fn parse_sha256_from_keytool_returns_normalized_hex() {
        let text = "SHA256: AA:BB:CC:DD:11:22:33:44:55:66:77:88:99:AA:BB:CC:DD:EE:FF:00:11:22:33:44:55:66:77:88:99:AA:BB:CC";
        assert_eq!(
            parse_sha256_from_keytool(text).as_deref(),
            Some("AABBCCDD112233445566778899AABBCCDDEEFF00112233445566778899AABBCC")
        );
    }

    #[test]
    fn parse_sha256_from_apksigner_returns_hex() {
        let text = "Signer #1 certificate SHA-256 digest: AABBCCDD112233445566778899AABBCCDDEEFF00112233445566778899AABBCC";
        assert_eq!(
            parse_sha256_from_apksigner(text).as_deref(),
            Some("AABBCCDD112233445566778899AABBCCDDEEFF00112233445566778899AABBCC")
        );
    }

    #[test]
    fn normalize_fingerprint_strips_colons_and_spaces() {
        assert_eq!(normalize_fingerprint("aa:bb cc"), "AABBCC");
    }
}

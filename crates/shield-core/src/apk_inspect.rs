use anyhow::{Context, Result};
use std::fs;
use std::io;
use std::io::Read;
use std::path::Path;

use crate::utils::{find_apksigner, find_java, find_keytool, no_window_command};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApkCheckOutcome {
    pub already_protected: bool,
    pub is_signed: bool,
    pub native_abis: Vec<String>,
}

pub fn check_apk(path: &Path, apksigner_path: Option<&Path>) -> Result<ApkCheckOutcome> {
    let file = fs::File::open(path)
        .map_err(|err| anyhow::anyhow!("{}", classify_apk_open_error(path, &err)))?;
    let mut archive = zip::ZipArchive::new(file)
        .map_err(|err| anyhow::anyhow!("{}", classify_apk_zip_error(path, &err)))?;

    let mut already_protected = false;
    let mut native_abis = Vec::new();
    for i in 0..archive.len() {
        let mut entry = match archive.by_index(i) {
            Ok(entry) => entry,
            Err(_) => continue,
        };

        if entry.name() == "classes.dex" && !already_protected {
            let entry_size = entry.size();
            already_protected = contains_valid_mshd_block(&mut entry, entry_size)
                .context("读取 classes.dex 失败")?;
        }
        if let Some(abi) = entry
            .name()
            .strip_prefix("lib/")
            .and_then(|name| name.split_once('/').map(|(abi, _)| abi))
        {
            if !native_abis.iter().any(|item| item == abi) {
                native_abis.push(abi.to_string());
            }
        }
    }
    native_abis.sort();

    Ok(ApkCheckOutcome {
        already_protected,
        is_signed: check_apk_signed(path, apksigner_path)?,
        native_abis,
    })
}

fn contains_valid_mshd_block(reader: &mut impl Read, total_size: u64) -> Result<bool> {
    const MSHD_HEADER_LEN: usize = 8;
    const OVERLAP_LEN: usize = MSHD_HEADER_LEN - 1;
    const BUFFER_LEN: usize = 64 * 1024;

    if total_size < MSHD_HEADER_LEN as u64 {
        return Ok(false);
    }

    let mut buffer = vec![0u8; BUFFER_LEN + OVERLAP_LEN];
    let mut overlap_len = 0usize;
    let mut consumed = 0u64;

    loop {
        let read_len = reader.read(&mut buffer[overlap_len..overlap_len + BUFFER_LEN])?;
        if read_len == 0 {
            return Ok(false);
        }

        let available = overlap_len + read_len;
        let base_offset = consumed.saturating_sub(overlap_len as u64);
        for index in 0..=available.saturating_sub(MSHD_HEADER_LEN) {
            if &buffer[index..index + 4] != b"MSHD" {
                continue;
            }
            let payload_len = u32::from_le_bytes(
                buffer[index + 4..index + MSHD_HEADER_LEN]
                    .try_into()
                    .expect("MSHD 长度字段固定为 4 字节"),
            ) as u64;
            let block_start = base_offset + index as u64;
            if block_start
                .checked_add(MSHD_HEADER_LEN as u64)
                .and_then(|offset| offset.checked_add(payload_len))
                == Some(total_size)
            {
                return Ok(true);
            }
        }

        consumed += read_len as u64;
        overlap_len = available.min(OVERLAP_LEN);
        buffer.copy_within(available - overlap_len..available, 0);
    }
}

fn classify_apk_open_error(path: &Path, err: &io::Error) -> String {
    match err.kind() {
        io::ErrorKind::NotFound => format!("找不到 APK 文件: {}", path.display()),
        io::ErrorKind::PermissionDenied => {
            format!("没有权限读取 APK 文件: {}", path.display())
        }
        _ => format!("无法打开 APK 文件: {}: {err}", path.display()),
    }
}

fn classify_apk_zip_error(path: &Path, err: &zip::result::ZipError) -> String {
    let meta_len = fs::metadata(path).map(|meta| meta.len()).ok();
    let raw = err.to_string();
    let lower = raw.to_lowercase();

    let reason = if meta_len == Some(0) {
        "文件为空，不是有效 APK"
    } else if lower.contains("invalid archive")
        || lower.contains("invalid zip")
        || lower.contains("could not find central directory")
        || lower.contains("eof")
    {
        "文件不是有效 APK/ZIP，或 APK 已损坏"
    } else if lower.contains("unsupported") {
        "APK ZIP 使用了当前工具暂不支持的压缩结构"
    } else {
        "无法解析 APK ZIP 结构"
    };

    format!("{reason}: {}。ZIP 错误：{raw}", path.display())
}

pub fn extract_apk_cert_fingerprint(
    apk_path: &Path,
    apksigner_path: Option<&Path>,
) -> Result<String> {
    let signer = match apksigner_path {
        Some(path) if path.exists() => path.to_path_buf(),
        Some(path) => anyhow::bail!("配置的 apksigner.jar 路径不存在: {}", path.display()),
        None => find_apksigner().context("提取 APK 签名证书需要 apksigner.jar")?,
    };
    let java = find_java().context("提取 APK 签名证书需要 Java")?;
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

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let detail = if stderr.trim().is_empty() {
            stdout.trim()
        } else {
            stderr.trim()
        };
        anyhow::bail!("APK 签名验证失败：{detail}")
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    select_single_apk_signer_fingerprint(&stdout)
}

pub fn extract_keystore_cert_fingerprint(
    ks: &Path,
    alias: &str,
    ks_pass: &str,
    ks_type: Option<&str>,
) -> Result<String> {
    let keytool = find_keytool()?;

    let mut args = vec![
        "-J-Duser.language=en",
        "-J-Duser.country=US",
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
        anyhow::bail!("keytool 已执行成功，但未返回可识别的 SHA-256 指纹")
    }

    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let detail = if stderr.trim().is_empty() {
        stdout.trim()
    } else {
        stderr.trim()
    };
    anyhow::bail!("无法读取 keystore 证书指纹：{detail}")
}

pub fn normalize_fingerprint(fp: &str) -> String {
    fp.to_uppercase().replace([':', ' '], "")
}

fn check_apk_signed(apk_path: &Path, apksigner_path: Option<&Path>) -> Result<bool> {
    let signer = apksigner_path
        .map(Path::to_path_buf)
        .or_else(|| find_apksigner().ok());
    if let Some(signer) = signer {
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
            let clean = after_label.replace([':', ' '], "").to_uppercase();
            if clean.len() == 64 && clean.chars().all(|c| c.is_ascii_hexdigit()) {
                return Some(clean);
            }
        }
    }
    None
}

fn parse_cert_sha256_from_apksigner(output: &str) -> Vec<String> {
    let mut fingerprints = Vec::new();
    for line in output.lines() {
        let upper = line.trim().to_uppercase();
        if upper.contains("CERTIFICATE SHA-256 DIGEST") && !upper.contains("SOURCE STAMP") {
            let Some(pos) = line.rfind(':') else {
                continue;
            };
            let hex = line[pos + 1..].trim().to_uppercase();
            if hex.len() == 64
                && hex.chars().all(|c| c.is_ascii_hexdigit())
                && !fingerprints.contains(&hex)
            {
                fingerprints.push(hex);
            }
        }
    }
    fingerprints
}

fn select_single_apk_signer_fingerprint(output: &str) -> Result<String> {
    let fingerprints = parse_cert_sha256_from_apksigner(output);
    match fingerprints.as_slice() {
        [fingerprint] => Ok(fingerprint.clone()),
        [] => anyhow::bail!("apksigner 未返回可识别的 APK 签名证书 SHA-256 指纹"),
        _ => anyhow::bail!(
            "检测到 {} 个 APK 内容签名证书；当前 DEXB v5 仅支持单签名 APK",
            fingerprints.len()
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        classify_apk_open_error, classify_apk_zip_error, contains_valid_mshd_block,
        normalize_fingerprint, parse_cert_sha256_from_apksigner, parse_sha256_from_keytool,
        select_single_apk_signer_fingerprint,
    };
    use std::io::{self, Cursor};
    use std::path::Path;

    #[test]
    fn parse_sha256_from_keytool_returns_normalized_hex() {
        let text = "SHA256: AA:BB:CC:DD:11:22:33:44:55:66:77:88:99:AA:BB:CC:DD:EE:FF:00:11:22:33:44:55:66:77:88:99:AA:BB:CC";
        assert_eq!(
            parse_sha256_from_keytool(text).as_deref(),
            Some("AABBCCDD112233445566778899AABBCCDDEEFF00112233445566778899AABBCC")
        );
    }

    #[test]
    fn parse_sha256_from_chinese_keytool_output() {
        let text = "证书指纹:\n\t SHA256: aa:bb:cc:dd:11:22:33:44:55:66:77:88:99:aa:bb:cc:dd:ee:ff:00:11:22:33:44:55:66:77:88:99:aa:bb:cc";
        assert_eq!(
            parse_sha256_from_keytool(text).as_deref(),
            Some("AABBCCDD112233445566778899AABBCCDDEEFF00112233445566778899AABBCC")
        );
    }

    #[test]
    fn parse_apksigner_单签名返回证书指纹() {
        let text = "Signer #1 certificate SHA-256 digest: AABBCCDD112233445566778899AABBCCDDEEFF00112233445566778899AABBCC";
        assert_eq!(
            select_single_apk_signer_fingerprint(text).unwrap(),
            "AABBCCDD112233445566778899AABBCCDDEEFF00112233445566778899AABBCC"
        );
    }

    #[test]
    fn parse_apksigner_兼容_v3_输出并忽略公钥摘要() {
        let text = "\
V3.0 Signer: certificate SHA-256 digest: 1111111111111111111111111111111111111111111111111111111111111111
V3.0 Signer: public key SHA-256 digest: 2222222222222222222222222222222222222222222222222222222222222222";
        assert_eq!(
            select_single_apk_signer_fingerprint(text).unwrap(),
            "1111111111111111111111111111111111111111111111111111111111111111"
        );
    }

    #[test]
    fn parse_apksigner_多签名明确拒绝() {
        let text = "\
V2 Signer #1: certificate SHA-256 digest: 1111111111111111111111111111111111111111111111111111111111111111
V2 Signer #2: certificate SHA-256 digest: 2222222222222222222222222222222222222222222222222222222222222222";
        let error = select_single_apk_signer_fingerprint(text).unwrap_err();
        assert!(error.to_string().contains("仅支持单签名 APK"));
    }

    #[test]
    fn parse_apksigner_忽略来源戳证书() {
        let text = "\
Signer #1 certificate SHA-256 digest: 1111111111111111111111111111111111111111111111111111111111111111
Source Stamp Signer certificate SHA-256 digest: 2222222222222222222222222222222222222222222222222222222222222222";
        assert_eq!(
            parse_cert_sha256_from_apksigner(text),
            vec!["1111111111111111111111111111111111111111111111111111111111111111"]
        );
    }

    #[test]
    fn normalize_fingerprint_strips_colons_and_spaces() {
        assert_eq!(normalize_fingerprint("aa:bb cc"), "AABBCC");
    }

    #[test]
    fn normalize_fingerprint_handles_lowercase_colons_and_spaces() {
        assert_eq!(normalize_fingerprint("aa:bb:cc dd ee ff"), "AABBCCDDEEFF");
    }

    #[test]
    fn parse_returns_none_without_sha256_digest() {
        assert_eq!(parse_sha256_from_keytool("SHA1: AA:BB"), None);
        assert!(
            parse_cert_sha256_from_apksigner("Signer #1 certificate SHA-1 digest: AABB").is_empty()
        );
    }

    #[test]
    fn apk_open_error_文件不存在提示更明确() {
        let message = classify_apk_open_error(
            Path::new("/tmp/missing.apk"),
            &io::ErrorKind::NotFound.into(),
        );
        assert!(message.starts_with("找不到 APK 文件"));
    }

    #[test]
    fn apk_zip_error_空文件提示更明确() {
        let dir = tempfile::tempdir().unwrap();
        let apk = dir.path().join("empty.apk");
        std::fs::write(&apk, []).unwrap();
        let message = classify_apk_zip_error(&apk, &zip::result::ZipError::InvalidArchive("eof"));
        assert!(message.starts_with("文件为空，不是有效 APK"));
    }

    #[test]
    fn mshd_载荷超过四千字节仍可识别() {
        let payload = vec![0x5a; 8192];
        let mut dex = vec![0u8; 128];
        dex.extend_from_slice(b"MSHD");
        dex.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        dex.extend_from_slice(&payload);

        assert!(contains_valid_mshd_block(&mut Cursor::new(&dex), dex.len() as u64).unwrap());
    }

    #[test]
    fn mshd_头跨读取分块仍可识别() {
        let payload = vec![0x3c; 32];
        let mut dex = vec![0u8; 64 * 1024 - 3];
        dex.extend_from_slice(b"MSHD");
        dex.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        dex.extend_from_slice(&payload);

        assert!(contains_valid_mshd_block(&mut Cursor::new(&dex), dex.len() as u64).unwrap());
    }

    #[test]
    fn mshd_长度未指向文件末尾不会误判() {
        let mut dex = b"dex-content-MSHD".to_vec();
        dex.extend_from_slice(&4u32.to_le_bytes());
        dex.extend_from_slice(b"data-extra");

        assert!(!contains_valid_mshd_block(&mut Cursor::new(&dex), dex.len() as u64).unwrap());
    }
}

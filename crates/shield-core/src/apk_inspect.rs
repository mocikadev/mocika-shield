use anyhow::{Context, Result};
use std::fs;
use std::io;
use std::io::Read;
use std::path::Path;

#[cfg(not(windows))]
use crate::utils::{find_apksigner, find_java, java_command};
mod signature;
pub use signature::{extract_apk_cert_fingerprint, extract_apk_cert_fingerprint_with_context};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApkCheckOutcome {
    pub already_protected: bool,
    pub is_signed: bool,
    pub native_abis: Vec<String>,
}

pub fn check_apk(path: &Path, apksigner_path: Option<&Path>) -> Result<ApkCheckOutcome> {
    #[cfg(windows)]
    return check_apk_default(path, apksigner_path);
    #[cfg(not(windows))]
    legacy_check(path, apksigner_path)
}

#[cfg(not(windows))]
fn legacy_check(path: &Path, apksigner_path: Option<&Path>) -> Result<ApkCheckOutcome> {
    let (already_protected, native_abis) = inspect_archive(path)?;
    Ok(ApkCheckOutcome {
        already_protected,
        is_signed: check_apk_signed(path, apksigner_path)?,
        native_abis,
    })
}

#[cfg(any(windows, test))]
pub(crate) fn check_apk_default(
    path: &Path,
    apksigner_path: Option<&Path>,
) -> Result<ApkCheckOutcome> {
    // 先沿原路径检查归档，损坏文件不启动 Java，也不退回 ZIP 签名标记猜测。
    inspect_archive(path)?;
    let signer = signature::selected_signer(apksigner_path)?;
    let session = crate::java_adapter::LocalSession::temporary()?;
    let result = check_apk_with_context(&session.context, &session.java, path, &signer);
    session.finish(result)
}

/// 显式工具检查；仅原验签器明确报告无有效签名时返回 false，工具故障仍为错误。
pub fn check_apk_with_context(
    context: &crate::JavaToolContext,
    java: &Path,
    path: &Path,
    signer: &Path,
) -> Result<ApkCheckOutcome> {
    let (already_protected, native_abis) = inspect_archive(path)?;
    let path = crate::utils::strip_unc_prefix(&std::path::absolute(path)?);
    let signer = crate::utils::strip_unc_prefix(&std::path::absolute(signer)?);
    let output = context.run_apksigner(
        java,
        &signer,
        &[
            "verify",
            path.to_str()
                .ok_or_else(|| anyhow::anyhow!("APK 路径无效"))?,
        ],
        None,
    )?;
    let rejected = output.status.code() == Some(1)
        && output
            .stderr
            .bytes
            .split(|byte| *byte == b'\n')
            .any(|line| line == b"DOES NOT VERIFY" || line == b"DOES NOT VERIFY\r");
    if output.stdout.truncated || output.stderr.truncated || (!output.status.success() && !rejected)
    {
        return Err(crate::signing::fixed_failure(
            &output,
            "APK 验签工具失败或结果无效",
        ));
    }
    Ok(ApkCheckOutcome {
        already_protected,
        is_signed: output.status.success(),
        native_abis,
    })
}

fn inspect_archive(path: &Path) -> Result<(bool, Vec<String>)> {
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

    Ok((already_protected, native_abis))
}

fn contains_valid_mshd_block(reader: &mut impl Read, total_size: u64) -> Result<bool> {
    Ok(crate::mshd::scan(reader, total_size, true, &mut || Ok(()))?
        .block
        .is_some())
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

pub fn extract_keystore_cert_fingerprint(
    ks: &Path,
    alias: &str,
    ks_pass: &str,
    ks_type: Option<&str>,
) -> Result<String> {
    crate::keytool::extract_keystore_cert_fingerprint(ks, alias, ks_pass, ks_type)
}

pub fn normalize_fingerprint(fp: &str) -> String {
    fp.to_uppercase().replace([':', ' '], "")
}

#[cfg(not(windows))]
fn check_apk_signed(apk_path: &Path, apksigner_path: Option<&Path>) -> Result<bool> {
    let signer = apksigner_path
        .map(Path::to_path_buf)
        .or_else(|| find_apksigner().ok());
    if let Some(signer) = signer {
        let java = find_java()?;
        if let Ok(status) = java_command(&java)
            .args([
                "-jar",
                signer.to_str().unwrap_or(""),
                "verify",
                apk_path.to_str().unwrap_or(""),
            ])
            // 此处只消费退出状态；原始输出不能继承并污染调用方的 JSON 协议。
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
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

#[cfg(not(windows))]
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

#[cfg(test)]
mod tests {
    use super::signature::{
        parse_cert_sha256_from_apksigner, select_single_apk_signer_fingerprint,
    };
    use super::{
        classify_apk_open_error, classify_apk_zip_error, contains_valid_mshd_block,
        normalize_fingerprint,
    };
    use crate::keytool::parse_sha256_from_keytool;
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

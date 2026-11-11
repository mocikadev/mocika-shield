//! APK 内容签名证书读取；只返回单签名指纹，不承担签名输出或任务提交。
#[cfg(not(windows))]
use crate::utils::{find_java, java_command};
use crate::{
    diagnostic::{Diagnostic, FailureCode, ToolName},
    utils::find_apksigner,
    JavaToolContext,
};
#[cfg(not(windows))]
use anyhow::Context;
use anyhow::Result;
use std::path::Path;

pub fn extract_apk_cert_fingerprint(
    apk_path: &Path,
    apksigner_path: Option<&Path>,
) -> Result<String> {
    #[cfg(windows)]
    {
        let signer = selected_signer(apksigner_path)?;
        let session = crate::java_adapter::LocalSession::temporary()?;
        let result = extract_apk_cert_fingerprint_with_context(
            &session.context,
            &session.java,
            apk_path,
            &signer,
        );
        session.finish(result)
    }
    #[cfg(not(windows))]
    legacy_fingerprint(apk_path, apksigner_path)
}

#[cfg(any(windows, test))]
pub(super) fn selected_signer(custom: Option<&Path>) -> Result<std::path::PathBuf> {
    let path = match custom {
        Some(path) => path.to_path_buf(),
        None => find_apksigner()
            .map_err(|error| Diagnostic::new(FailureCode::ToolNotFound).attach(error))?,
    };
    if !path.is_file() {
        anyhow::bail!("配置的 apksigner.jar 不存在或不是普通文件");
    }
    Ok(crate::utils::strip_unc_prefix(&std::path::absolute(path)?))
}

#[cfg(not(windows))]
fn legacy_fingerprint(apk_path: &Path, apksigner_path: Option<&Path>) -> Result<String> {
    let signer = match apksigner_path {
        Some(path) if path.exists() => path.to_path_buf(),
        Some(path) => anyhow::bail!("配置的 apksigner.jar 路径不存在: {}", path.display()),
        None => find_apksigner().context("提取 APK 签名证书需要 apksigner.jar")?,
    };
    let java = find_java().context("提取 APK 签名证书需要 Java")?;
    let output = java_command(&java)
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
        return Err(Diagnostic::from_tool_streams(
            ToolName::Apksigner,
            output.status.code(),
            &output.stdout,
            &output.stderr,
        )
        .attach(anyhow::anyhow!("APK 签名验证失败：{detail}")));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    select_single_apk_signer_fingerprint(&stdout)
}

/// 固定工具的显式验签入口；不从全局选择 Java／JAR，不回传原始日志。
pub fn extract_apk_cert_fingerprint_with_context(
    context: &JavaToolContext,
    java: &Path,
    apk_path: &Path,
    apksigner: &Path,
) -> Result<String> {
    let apk = crate::utils::strip_unc_prefix(&std::path::absolute(apk_path)?);
    let jar = crate::utils::strip_unc_prefix(&std::path::absolute(apksigner)?);
    let apk = apk
        .to_str()
        .ok_or_else(|| anyhow::anyhow!("APK 路径包含非法字符"))?;
    let output = context.run_apksigner(java, &jar, &["verify", "--print-certs", apk], None)?;
    if !output.status.success() || output.stdout.truncated || output.stderr.truncated {
        return Err(crate::signing::fixed_failure(
            &output,
            "APK 签名验证失败或结果超出预算",
        ));
    }
    let text = std::str::from_utf8(&output.stdout.bytes).map_err(|_| {
        let mut diagnostic = Diagnostic::new(FailureCode::ToolOutputEncodingInvalid);
        diagnostic.tool = Some(ToolName::Apksigner);
        diagnostic.exit_code = output.status.code();
        diagnostic.attach(anyhow::anyhow!(
            "APK 验签结果不是有效 UTF-8，未返回部分指纹"
        ))
    })?;
    select_single_apk_signer_fingerprint(text)
}

pub(super) fn parse_cert_sha256_from_apksigner(output: &str) -> Vec<String> {
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

pub(super) fn select_single_apk_signer_fingerprint(output: &str) -> Result<String> {
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

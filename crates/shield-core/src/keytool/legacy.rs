//! 保留原 keytool 只读调用和文本协议；不依赖 services、数据库或界面。
#[cfg(not(windows))]
use super::keytool_command;
#[cfg(not(windows))]
use crate::{
    diagnostic::{Diagnostic, FailureCode, ToolName},
    utils::find_keytool,
};
#[cfg(not(windows))]
use anyhow::Context;
use anyhow::Result;
#[cfg(not(windows))]
use std::path::Path;

#[cfg(not(windows))]
pub(super) fn aliases(path: &Path, password: &str, kind: Option<&str>) -> Result<Vec<String>> {
    let keytool =
        find_keytool().map_err(|error| Diagnostic::new(FailureCode::ToolNotFound).attach(error))?;
    let output = keytool_command(&keytool)
        .args([
            "-list",
            "-keystore",
            path.to_str().unwrap_or(""),
            "-storetype",
            kind.unwrap_or("JKS"),
            "-storepass:env",
            "MOCIKA_SHIELD_CERT_KS_PASS",
        ])
        .env("MOCIKA_SHIELD_CERT_KS_PASS", password)
        .output()
        .map_err(|error| {
            Diagnostic::from_io(&error).attach(anyhow::anyhow!(
                "启动 keytool 失败，请确认 JDK 8+ 已安装: {error}"
            ))
        })?;
    if !output.status.success() {
        return Err(Diagnostic::from_tool_streams(
            ToolName::Keytool,
            output.status.code(),
            &output.stdout,
            &output.stderr,
        )
        .attach(anyhow::anyhow!(classify_keytool_error(
            &String::from_utf8_lossy(&output.stderr)
        ))));
    }
    let aliases = parse_keytool_aliases(&output.stdout).map_err(|message| {
        Diagnostic::new(FailureCode::ToolOutputEncodingInvalid).attach(anyhow::anyhow!(message))
    })?;
    if aliases.is_empty() {
        return Err(Diagnostic::new(FailureCode::KeyAliasNotFound)
            .attach(anyhow::anyhow!("未在 keystore 中找到任何 alias")));
    }
    Ok(aliases)
}

fn parse_keytool_aliases(output: &[u8]) -> Result<Vec<String>, String> {
    let output = std::str::from_utf8(output).map_err(|_| {
        "keytool 输出不是有效 UTF-8，无法安全识别 Key Alias；请检查 Java 环境，不要保存乱码别名"
            .to_string()
    })?;
    let mut aliases = Vec::new();
    for line in output.lines() {
        let trimmed = line.trim();
        if trimmed.contains("PrivateKeyEntry")
            || trimmed.contains("SecretKeyEntry")
            || trimmed.contains("trustedCertEntry")
        {
            if let Some(alias) = trimmed.split(',').next() {
                let alias = alias.trim().to_string();
                if !alias.is_empty() {
                    aliases.push(alias);
                }
            }
        }
    }
    Ok(aliases)
}

fn classify_keytool_error(stderr: &str) -> String {
    let raw = stderr.trim();
    let lower = raw.to_lowercase();

    let reason = if lower.contains("password was incorrect")
        || lower.contains("password is incorrect")
        || lower.contains("tampered with, or password was incorrect")
        || raw.contains("密码不正确")
        || raw.contains("口令不正确")
        || raw.contains("密码错误")
    {
        "Keystore 密码不正确"
    } else if lower.contains("invalid keystore format")
        || lower.contains("unrecognized keystore format")
        || lower.contains("toderinputstream rejects tag type")
        || lower.contains("derinputstream.getlength")
        || raw.contains("无效的密钥库格式")
        || raw.contains("无法识别的密钥库格式")
    {
        "证书格式可能不是当前选择的 JKS/PKCS12，或文件已经损坏"
    } else if lower.contains("no such file")
        || lower.contains("cannot find")
        || lower.contains("系统找不到")
        || raw.contains("没有那个文件")
    {
        "找不到 Keystore 文件，请确认文件仍在原路径"
    } else if lower.contains("permission denied") || raw.contains("权限") {
        "没有权限读取 Keystore 文件"
    } else {
        "无法读取证书文件，请确认它是有效的 JKS 或 PKCS12 keystore"
    };

    reason.to_string()
}

#[cfg(not(windows))]
pub(super) fn fingerprint(
    ks: &Path,
    alias: &str,
    ks_pass: &str,
    ks_type: Option<&str>,
) -> Result<String> {
    let keytool =
        find_keytool().map_err(|err| Diagnostic::new(FailureCode::ToolNotFound).attach(err))?;

    let mut args = vec![
        "-list",
        "-v",
        "-keystore",
        ks.to_str().unwrap_or(""),
        "-alias",
        alias,
        "-storepass:env",
        crate::java_process::KS_PASS_ENV,
    ];
    if let Some(kind) = ks_type {
        args.push("-storetype");
        args.push(kind);
    }

    let output = keytool_command(&keytool)
        .args(&args)
        .env(crate::java_process::KS_PASS_ENV, ks_pass)
        .output()
        .context("执行 keytool 失败")?;

    if output.status.success() {
        let stdout = std::str::from_utf8(&output.stdout)
            .context("keytool 输出不是有效 UTF-8，无法安全读取证书指纹")
            .map_err(|err| Diagnostic::new(FailureCode::ToolOutputEncodingInvalid).attach(err))?;
        if let Some(fp) = parse_sha256_from_keytool(stdout) {
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
    Err(Diagnostic::from_tool_streams(
        ToolName::Keytool,
        output.status.code(),
        &output.stdout,
        &output.stderr,
    )
    .attach(anyhow::anyhow!("无法读取 keystore 证书指纹：{detail}")))
}

pub(crate) fn parse_sha256_from_keytool(output: &str) -> Option<String> {
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

#[path = "legacy_tests.rs"]
#[cfg(test)]
mod tests;

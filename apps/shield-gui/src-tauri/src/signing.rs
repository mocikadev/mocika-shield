use crate::app_config::normalize_keystore_type;
use crate::app_paths::find_apksigner_path;
use crate::cert_store::CertificateRecord;
use crate::failure_diagnostic::ExecutionFailure;
use shield_core::diagnostic::{Diagnostic, FailureCode, ToolName};
use shield_core::{
    keytool::keytool_command, sign_apk_with_progress as shield_sign_apk, utils::find_keytool,
    KeystoreType, SignOptions, SigningProgressStep, SigningVersions,
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
    .map_err(|e| ExecutionFailure::diagnosed(e.to_string(), Diagnostic::from_error(&e)))?;
    Ok(())
}

pub(crate) fn query_keystore_aliases(
    keystore_path: String,
    ks_pass: String,
    ks_type: Option<String>,
) -> Result<Vec<String>, ExecutionFailure> {
    let ks_type_str = ks_type.as_deref().unwrap_or("JKS");
    let keytool = find_keytool().map_err(|err| {
        ExecutionFailure::diagnosed(err.to_string(), Diagnostic::new(FailureCode::ToolNotFound))
    })?;
    let output = keytool_command(&keytool)
        .args([
            "-list",
            "-keystore",
            &keystore_path,
            "-storetype",
            ks_type_str,
            "-storepass",
            &ks_pass,
        ])
        .output()
        .map_err(|e| {
            ExecutionFailure::diagnosed(
                format!("启动 keytool 失败，请确认 JDK 8+ 已安装: {e}"),
                Diagnostic::from_io(&e),
            )
        })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(ExecutionFailure::diagnosed(
            classify_keytool_error(&stderr),
            Diagnostic::from_tool_output(
                ToolName::Keytool,
                output.status.code(),
                if output.stderr.is_empty() {
                    &output.stdout
                } else {
                    &output.stderr
                },
            ),
        ));
    }

    let aliases = parse_keytool_aliases(&output.stdout).map_err(|message| {
        ExecutionFailure::diagnosed(
            message,
            Diagnostic::new(FailureCode::ToolOutputEncodingInvalid),
        )
    })?;

    if aliases.is_empty() {
        Err(ExecutionFailure::diagnosed(
            "未在 keystore 中找到任何 alias".to_string(),
            Diagnostic::new(FailureCode::KeyAliasNotFound),
        ))
    } else {
        Ok(aliases)
    }
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

    if raw.is_empty() {
        reason.to_string()
    } else {
        format!("{reason}。keytool 输出：{raw}")
    }
}

#[cfg(test)]
mod tests {
    use super::{classify_keytool_error, parse_keytool_aliases};

    #[test]
    fn keytool_密码错误提示更明确() {
        let message = classify_keytool_error(
            "keytool error: java.io.IOException: keystore password was incorrect",
        );
        assert!(message.starts_with("Keystore 密码不正确"));
    }

    #[test]
    fn keytool_旧版密码错误提示更明确() {
        let message = classify_keytool_error(
            "keytool error: java.io.IOException: Keystore was tampered with, or password was incorrect",
        );
        assert!(message.starts_with("Keystore 密码不正确"));
    }

    #[test]
    fn keytool_格式不匹配提示更明确() {
        let message =
            classify_keytool_error("keytool error: java.io.IOException: Invalid keystore format");
        assert!(message.starts_with("证书格式可能不是当前选择的 JKS/PKCS12"));
    }

    #[test]
    fn keytool_文件不存在提示更明确() {
        let message =
            classify_keytool_error("keytool error: java.io.FileNotFoundException: no such file");
        assert!(message.starts_with("找不到 Keystore 文件"));
    }

    #[test]
    fn keytool_未知错误保留原始输出摘要() {
        let message = classify_keytool_error("keytool error: unknown failure");
        assert!(message.starts_with("无法读取证书文件"));
        assert!(message.contains("unknown failure"));
    }

    #[test]
    fn 解析_keytool_alias_保持原有行为() {
        let output = "\
release, 2026年7月9日, PrivateKeyEntry,
trusted, 2026年7月9日, trustedCertEntry,";
        assert_eq!(
            parse_keytool_aliases(output.as_bytes()).unwrap(),
            vec!["release", "trusted"]
        );
    }

    #[test]
    fn 中文_alias_按_utf8_完整保留() {
        let output = "中文签名pos, Sep 16, 2026, PrivateKeyEntry,";
        assert_eq!(
            parse_keytool_aliases(output.as_bytes()).unwrap(),
            vec!["中文签名pos"]
        );
    }

    #[test]
    fn 非_utf8_alias_不得替换成乱码后返回() {
        let output = b"\xd6\xd0\xce\xc4pos, Sep 16, 2026, PrivateKeyEntry,";
        assert!(parse_keytool_aliases(output).unwrap_err().contains("UTF-8"));
    }
}

use crate::app_config::normalize_keystore_type;
use crate::app_paths::find_apksigner_path;
use crate::cert_store::CertificateRecord;
use shield_core::{
    sign_apk_with_progress as shield_sign_apk,
    utils::{find_keytool, no_window_command},
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
) -> Result<(), String> {
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
    .map_err(|e| e.to_string())?;
    Ok(())
}

pub(crate) fn query_keystore_aliases(
    keystore_path: String,
    ks_pass: String,
    ks_type: Option<String>,
) -> Result<Vec<String>, String> {
    let ks_type_str = ks_type.as_deref().unwrap_or("JKS");
    let keytool = find_keytool().map_err(|err| err.to_string())?;
    let output = no_window_command(&keytool)
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
        .map_err(|e| format!("启动 keytool 失败，请确认 JDK 8+ 已安装: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(classify_keytool_error(&stderr));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let aliases = parse_keytool_aliases(&stdout);

    if aliases.is_empty() {
        Err("未在 keystore 中找到任何 alias".to_string())
    } else {
        Ok(aliases)
    }
}

fn parse_keytool_aliases(output: &str) -> Vec<String> {
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
    aliases
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
        assert_eq!(parse_keytool_aliases(output), vec!["release", "trusted"]);
    }
}

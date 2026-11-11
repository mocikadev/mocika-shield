//! 显式适配入口只输出固定提示与有限分类，绝不把原始工具日志附入错误。
use crate::{
    diagnostic::{Diagnostic, FailureCode, ToolName},
    java_adapter::JarOutput,
};

pub(crate) fn fixed_failure(output: &JarOutput, message: &'static str) -> anyhow::Error {
    let mut diagnostic = Diagnostic::from_tool_streams(
        ToolName::Apksigner,
        output.status.code(),
        &output.stdout.bytes,
        &output.stderr.bytes,
    );
    // 选定原 JAR 的缺失／非密钥 Alias 分支实际使用这组措辞，不误认其他私钥算法错误。
    if diagnostic.code == FailureCode::ToolProcessFailed
        && [&output.stdout.bytes, &output.stderr.bytes]
            .iter()
            .any(|bytes| {
                std::str::from_utf8(bytes).is_ok_and(|text| {
                    let lower = text.to_ascii_lowercase();
                    lower.contains(" entry \"") && lower.contains("\" does not contain a key")
                })
            })
    {
        diagnostic.code = FailureCode::KeyAliasNotFound;
    }
    let truncated = output.stdout.truncated || output.stderr.truncated;
    diagnostic.attach(anyhow::anyhow!("{message}（退出码 {:?}，诊断已截断：{truncated}）；请核对输入与签名资料，未回退旧参数重试。", output.status.code()))
}

pub(super) fn classify_apksigner_error(stderr: &str, stdout: &str) -> String {
    let raw = sanitize_tool_output(if stderr.trim().is_empty() {
        stdout
    } else {
        stderr
    });
    let lower = raw.to_lowercase();

    let reason = if lower.contains("keystore was tampered with")
        || lower.contains("password was incorrect")
        || lower.contains("password is incorrect")
        || lower.contains("keystore password was incorrect")
        || raw.contains("密码不正确")
        || raw.contains("口令不正确")
        || raw.contains("密码错误")
    {
        "Keystore 密码不正确，请检查证书密码"
    } else if lower.contains("cannot recover key")
        || lower.contains("failed to recover key")
        || lower.contains("key password was incorrect")
        || lower.contains("given final block not properly padded")
        || raw.contains("无法恢复密钥")
    {
        "Key 密码不正确；如果 Key 密码与 Keystore 密码相同，可在证书配置中留空 Key 密码"
    } else if lower.contains("no key with alias")
        || lower.contains("alias") && lower.contains("does not exist")
        || lower.contains("failed to find")
        || raw.contains("别名") && raw.contains("不存在")
    {
        "证书 Alias 不存在，请在证书页重新识别 Alias 或重新导入证书"
    } else if lower.contains("invalid keystore format")
        || lower.contains("unrecognized keystore format")
        || lower.contains("toderinputstream rejects tag type")
        || lower.contains("derinputstream.getlength")
        || raw.contains("无效的密钥库格式")
        || raw.contains("无法识别的密钥库格式")
    {
        "证书格式与选择的 JKS/PKCS12 类型不匹配，或 keystore 文件已损坏"
    } else if lower.contains("no such file")
        || lower.contains("file not found")
        || lower.contains("cannot find")
        || raw.contains("系统找不到")
        || raw.contains("没有那个文件")
    {
        "找不到输入 APK、输出目录或 Keystore 文件，请确认文件仍在原路径"
    } else if lower.contains("permission denied")
        || lower.contains("access is denied")
        || raw.contains("权限")
        || raw.contains("拒绝访问")
    {
        "没有权限读取输入文件或写入输出文件，请检查文件权限与输出目录"
    } else if lower.contains("not a valid apk")
        || lower.contains("invalid apk")
        || lower.contains("malformed apk")
        || lower.contains("zip")
        || lower.contains("failed to parse")
    {
        "输入文件不是有效 APK，或 APK 结构已经损坏"
    } else if lower.contains("min-sdk-version")
        || lower.contains("minimum supported platform version")
        || lower.contains("failed to determine")
    {
        "无法识别 APK 的 minSdkVersion；请确认 AndroidManifest.xml 有效，必要时先用 Android 构建工具重新生成 APK"
    } else if lower.contains("java.lang.unsupportedclassversionerror")
        || lower.contains("unsupported major.minor version")
    {
        "当前 Java 版本过低，请安装并使用完整 JDK 8+"
    } else {
        "apksigner 未返回可识别的错误类型"
    };

    if raw.is_empty() {
        reason.to_string()
    } else {
        format!("{reason}。apksigner 输出：{raw}")
    }
}

fn sanitize_tool_output(raw: &str) -> String {
    raw.split_whitespace()
        .map(|part| {
            if part.starts_with("pass:") {
                "pass:******"
            } else {
                part
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 适配签名错误只返回固定分类退出码及截断状态() {
        #[cfg(unix)]
        let status = {
            use std::os::unix::process::ExitStatusExt;
            std::process::ExitStatus::from_raw(1 << 8)
        };
        #[cfg(windows)]
        let status = {
            use std::os::windows::process::ExitStatusExt;
            std::process::ExitStatus::from_raw(1)
        };
        let mut output = JarOutput {
            status,
            stdout: crate::java_adapter::Captured {
                bytes: Vec::new(),
                truncated: false,
            },
            stderr: crate::java_adapter::Captured {
                bytes: b"password was incorrect secret-that-must-not-leak /private/material"
                    .to_vec(),
                truncated: true,
            },
        };
        let error = fixed_failure(&output, "APK 签名失败");
        let diagnostic = Diagnostic::from_error(&error);
        assert_eq!(
            diagnostic.code,
            crate::diagnostic::FailureCode::KeystorePasswordInvalid
        );
        assert_eq!(diagnostic.tool, Some(ToolName::Apksigner));
        assert_eq!(diagnostic.exit_code, Some(1));
        assert!(error.to_string().contains("诊断已截断：true"));
        assert!(!format!("{error:#}").contains("secret-that-must-not-leak"));
        assert!(!format!("{error:#}").contains("/private/material"));
        for (message, code) in [
            (
                "fixture.jks entry \"hidden-alias\" does not contain a key",
                FailureCode::KeyAliasNotFound,
            ),
            (
                "fixture.jks entry \"hidden-alias\" does not contain a private key",
                FailureCode::ToolProcessFailed,
            ),
        ] {
            output.stderr.bytes = message.as_bytes().to_vec();
            let error = fixed_failure(&output, "APK 签名失败");
            assert_eq!(Diagnostic::from_error(&error).code, code);
            assert!(!format!("{error:#}").contains("hidden-alias"));
        }
    }
    #[test]
    fn apksigner_密码错误提示更明确() {
        let message = classify_apksigner_error(
            "Failed to load signer: Keystore was tampered with, or password was incorrect",
            "",
        );
        assert!(message.starts_with("Keystore 密码不正确"));
    }

    #[test]
    fn apksigner_key_密码错误提示更明确() {
        let message = classify_apksigner_error(
            "java.security.UnrecoverableKeyException: Cannot recover key",
            "",
        );
        assert!(message.starts_with("Key 密码不正确"));
    }

    #[test]
    fn apksigner_alias_不存在提示更明确() {
        let message = classify_apksigner_error("No key with alias release in keystore", "");
        assert!(message.starts_with("证书 Alias 不存在"));
    }

    #[test]
    fn apksigner_非法_apk_提示更明确() {
        let message = classify_apksigner_error("Failed to parse APK: not a valid APK", "");
        assert!(message.starts_with("输入文件不是有效 APK"));
    }

    #[test]
    fn apksigner_输出会脱敏_pass_参数() {
        let message = classify_apksigner_error("failed with --ks-pass pass:secret123", "");
        assert!(message.contains("pass:******"));
        assert!(!message.contains("secret123"));
    }
}

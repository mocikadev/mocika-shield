//! 旧新建工具参数和错误文本兼容；不负责资料发布或入库。
#[cfg(not(windows))]
use super::{keytool_command, CreateKeystoreOptions};
#[cfg(not(windows))]
use crate::{
    diagnostic::{Diagnostic, FailureCode, ToolName},
    utils::find_keytool,
};
#[cfg(not(windows))]
use anyhow::Result;
#[cfg(not(windows))]
use std::path::Path;

#[cfg(not(windows))]
pub(super) fn generate(output_path: &Path, options: &CreateKeystoreOptions<'_>) -> Result<()> {
    if std::fs::symlink_metadata(output_path).is_ok() {
        anyhow::bail!("暂存证书目标已存在，未调用工具或覆盖原件");
    }
    let keytool =
        find_keytool().map_err(|err| Diagnostic::new(FailureCode::ToolNotFound).attach(err))?;
    let effective_key_password = if options.key_password.trim().is_empty() {
        options.keystore_password
    } else {
        options.key_password
    };

    let output = keytool_command(&keytool)
        .args([
            "-genkeypair",
            "-keystore",
            output_path.to_string_lossy().as_ref(),
            "-storetype",
            options.kind,
            "-storepass:env",
            "MOCIKA_SHIELD_CERT_KS_PASS",
            "-alias",
            options.alias,
            "-keypass:env",
            "MOCIKA_SHIELD_CERT_KEY_PASS",
            "-dname",
            options.dname,
            "-validity",
            &options.validity_days.to_string(),
            "-keyalg",
            "RSA",
            "-keysize",
            &options.key_size.to_string(),
        ])
        .env("MOCIKA_SHIELD_CERT_KS_PASS", options.keystore_password)
        .env("MOCIKA_SHIELD_CERT_KEY_PASS", effective_key_password)
        .output()
        .map_err(|e| {
            Diagnostic::from_io(&e).attach(anyhow::anyhow!(
                "启动 keytool 失败，请确认 JDK 8+ 已安装: {e}"
            ))
        })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(Diagnostic::from_tool_streams(
            ToolName::Keytool,
            output.status.code(),
            &output.stdout,
            &output.stderr,
        )
        .attach(anyhow::anyhow!(classify_create_keystore_error(&stderr))));
    }

    Ok(())
}

fn classify_create_keystore_error(stderr: &str) -> String {
    let raw = stderr.trim();
    let lower = raw.to_lowercase();

    let reason = if lower.contains("alias") && lower.contains("already exists")
        || raw.contains("别名") && raw.contains("已经存在")
        || raw.contains("别名") && raw.contains("已存在")
    {
        "证书 Alias 已存在，请换一个 Alias 或文件名"
    } else if lower.contains("key password must be at least 6 characters")
        || lower.contains("password must be at least 6 characters")
        || lower.contains("password is too short")
        || raw.contains("密码至少")
        || raw.contains("口令至少")
    {
        "证书密码不符合 keytool 要求，Keystore 密码和 Key 密码至少需要 6 个字符"
    } else if lower.contains("incorrect avas format")
        || lower.contains("invalid name")
        || lower.contains("distinguished name")
        || raw.contains("专有名称")
        || raw.contains("名称无效")
    {
        "证书主题信息格式不正确，请检查 CN、OU、O、L、ST、C 等字段"
    } else if lower.contains("permission denied")
        || lower.contains("access is denied")
        || raw.contains("权限")
        || raw.contains("拒绝访问")
    {
        "没有权限写入证书文件，请检查应用数据目录权限"
    } else if lower.contains("no such file")
        || lower.contains("cannot find")
        || raw.contains("没有那个文件")
        || raw.contains("系统找不到")
    {
        "证书保存目录不存在或不可访问"
    } else {
        "创建证书失败，请检查证书名称、Alias、密码和主题信息"
    };

    reason.to_string()
}

#[cfg(test)]
mod tests {
    use super::classify_create_keystore_error;
    #[test]
    fn 创建证书_alias_已存在提示更明确() {
        let message = classify_create_keystore_error(
            "keytool error: java.lang.Exception: Alias <release> already exists",
        );
        assert!(message.starts_with("证书 Alias 已存在"));
    }

    #[test]
    fn 创建证书_密码过短提示更明确() {
        let message = classify_create_keystore_error(
            "keytool error: Key password must be at least 6 characters",
        );
        assert!(message.starts_with("证书密码不符合 keytool 要求"));
    }

    #[test]
    fn 创建证书_dname_非法提示更明确() {
        let message = classify_create_keystore_error(
            "keytool error: java.io.IOException: Incorrect AVAs format",
        );
        assert!(message.starts_with("证书主题信息格式不正确"));
    }

    #[test]
    fn 创建证书_目录权限错误提示更明确() {
        let message = classify_create_keystore_error(
            "keytool error: java.io.FileNotFoundException: Permission denied",
        );
        assert!(message.starts_with("没有权限写入证书文件"));
    }

    #[test]
    fn 创建证书_未知错误不回传原始工具输出() {
        let message = classify_create_keystore_error("keytool error: unknown failure");
        assert!(message.starts_with("创建证书失败"));
        assert!(!message.contains("unknown failure"));
    }
}

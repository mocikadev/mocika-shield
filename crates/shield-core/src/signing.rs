use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use tempfile::TempDir;

use crate::utils::{find_java, no_window_command};
use crate::zipalign::align_apk;

#[derive(Debug, Clone, PartialEq)]
pub enum KeystoreType {
    Jks,
    Pkcs12,
}

impl KeystoreType {
    pub fn as_str(&self) -> &'static str {
        match self {
            KeystoreType::Jks => "JKS",
            KeystoreType::Pkcs12 => "PKCS12",
        }
    }

    pub fn parse(s: &str) -> Self {
        match s.to_uppercase().as_str() {
            "PKCS12" | "P12" => KeystoreType::Pkcs12,
            _ => KeystoreType::Jks,
        }
    }
}

#[derive(Debug, Clone)]
pub struct SigningVersions {
    pub v1: bool,
    pub v2: bool,
    pub v3: bool,
    pub v4: bool,
}

impl Default for SigningVersions {
    fn default() -> Self {
        Self {
            v1: true,
            v2: true,
            v3: true,
            v4: false,
        }
    }
}

#[derive(Debug, Clone)]
pub struct SignOptions {
    pub apk_path: PathBuf,
    pub output_path: Option<PathBuf>,
    pub keystore_path: PathBuf,
    pub key_alias: String,
    pub keystore_password: String,
    pub key_password: String,
    pub apksigner_path: Option<PathBuf>,
    pub keystore_type: KeystoreType,
    pub signing_versions: SigningVersions,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SigningProgressStep {
    Prepare,
    Align,
    Sign,
}

pub fn sign_apk(opts: &SignOptions) -> Result<()> {
    sign_apk_with_progress(opts, |_| Ok(()))
}

pub fn sign_apk_with_progress(
    opts: &SignOptions,
    mut on_progress: impl FnMut(SigningProgressStep) -> std::result::Result<(), String>,
) -> Result<()> {
    on_progress(SigningProgressStep::Prepare).map_err(anyhow::Error::msg)?;
    let apksigner = match &opts.apksigner_path {
        Some(p) if p.exists() => p.clone(),
        Some(p) => anyhow::bail!("配置的 apksigner.jar 路径不存在: {}", p.display()),
        None => find_apksigner()?,
    };

    let java = find_java()?;

    let ks_path_str = opts
        .keystore_path
        .to_str()
        .context("keystore 路径包含非法字符")?;
    let apksigner_str = apksigner
        .to_str()
        .context("apksigner 路径包含非法字符")?
        .to_string();
    let ks_pass_arg = format!("pass:{}", opts.keystore_password);
    let key_pass_arg = format!("pass:{}", opts.key_password);

    let v = &opts.signing_versions;
    let temp_dir = TempDir::new().context("创建签名临时目录失败")?;
    let aligned_input = temp_dir.path().join("aligned.apk");
    on_progress(SigningProgressStep::Align).map_err(anyhow::Error::msg)?;
    std::fs::copy(&opts.apk_path, &aligned_input)
        .with_context(|| format!("复制待签名 APK 到临时目录失败: {}", opts.apk_path.display()))?;
    align_apk(&aligned_input).context("内置 APK 对齐失败")?;

    let final_output = opts
        .output_path
        .clone()
        .unwrap_or_else(|| opts.apk_path.clone());
    let sign_output = if final_output == opts.apk_path {
        temp_dir.path().join("signed.apk")
    } else {
        final_output.clone()
    };

    let aligned_input_str = aligned_input
        .to_str()
        .context("临时 APK 路径包含非法字符")?;
    let sign_output_str = sign_output.to_str().context("签名输出路径包含非法字符")?;

    let mut cmd_args = vec![
        "-jar",
        &apksigner_str,
        "sign",
        "--ks-type",
        opts.keystore_type.as_str(),
        "--ks",
        ks_path_str,
        "--ks-key-alias",
        &opts.key_alias,
        "--ks-pass",
        &ks_pass_arg,
        "--key-pass",
        &key_pass_arg,
        "--v1-signing-enabled",
        bool_str(v.v1),
        "--v2-signing-enabled",
        bool_str(v.v2),
        "--v3-signing-enabled",
        bool_str(v.v3),
        "--out",
        sign_output_str,
    ];

    if v.v4 {
        cmd_args.push("--v4-signing-enabled");
        cmd_args.push("true");
    }

    cmd_args.push(aligned_input_str);

    on_progress(SigningProgressStep::Sign).map_err(anyhow::Error::msg)?;
    let output = no_window_command(&java)
        .args(&cmd_args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .context("启动 apksigner 失败，请确认 Java 可用")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let code = output.status.code().unwrap_or(-1);
        anyhow::bail!(
            "签名失败（apksigner 退出码 {code}）：{}",
            classify_apksigner_error(&stderr, &stdout)
        );
    }

    if sign_output != final_output {
        if final_output.exists() {
            std::fs::remove_file(&final_output)
                .with_context(|| format!("删除旧的签名输出失败: {}", final_output.display()))?;
        }
        std::fs::rename(&sign_output, &final_output).with_context(|| {
            format!(
                "写回最终签名 APK 失败: {} -> {}",
                sign_output.display(),
                final_output.display()
            )
        })?;
    }

    Ok(())
}

fn bool_str(b: bool) -> &'static str {
    if b {
        "true"
    } else {
        "false"
    }
}

pub fn find_apksigner() -> Result<PathBuf> {
    crate::utils::find_apksigner()
}

pub fn check_apksigner(custom: Option<&Path>) -> (bool, Option<String>) {
    if let Some(p) = custom {
        if p.exists() {
            return (true, None);
        } else {
            return (
                false,
                Some(format!("配置的 apksigner.jar 路径不存在: {}", p.display())),
            );
        }
    }
    match find_apksigner() {
        Ok(_) => (true, None),
        Err(e) => (false, Some(e.to_string())),
    }
}

fn classify_apksigner_error(stderr: &str, stdout: &str) -> String {
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
        "当前 Java 版本过低，请安装并使用完整 JDK 17+"
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
    fn keystore_type_from_str_jks_is_default() {
        assert_eq!(KeystoreType::parse("JKS"), KeystoreType::Jks);
        assert_eq!(KeystoreType::parse("jks"), KeystoreType::Jks);
        assert_eq!(KeystoreType::parse("unknown"), KeystoreType::Jks);
    }

    #[test]
    fn keystore_type_from_str_pkcs12_variants() {
        assert_eq!(KeystoreType::parse("PKCS12"), KeystoreType::Pkcs12);
        assert_eq!(KeystoreType::parse("pkcs12"), KeystoreType::Pkcs12);
        assert_eq!(KeystoreType::parse("P12"), KeystoreType::Pkcs12);
        assert_eq!(KeystoreType::parse("p12"), KeystoreType::Pkcs12);
    }

    #[test]
    fn keystore_type_as_str_values() {
        assert_eq!(KeystoreType::Jks.as_str(), "JKS");
        assert_eq!(KeystoreType::Pkcs12.as_str(), "PKCS12");
    }

    #[test]
    fn signing_versions_default_enables_v1_v2_v3_not_v4() {
        let v = SigningVersions::default();
        assert!(v.v1);
        assert!(v.v2);
        assert!(v.v3);
        assert!(!v.v4);
    }

    #[test]
    fn bool_str_returns_correct_literals() {
        assert_eq!(bool_str(true), "true");
        assert_eq!(bool_str(false), "false");
    }

    #[test]
    fn check_apksigner_nonexistent_custom_path_returns_error() {
        let (ok, msg) = check_apksigner(Some(Path::new("/nonexistent/apksigner.jar")));
        assert!(!ok);
        assert!(msg.is_some());
    }

    #[test]
    fn check_apksigner_existing_custom_path_returns_ok() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let (ok, msg) = check_apksigner(Some(tmp.path()));
        assert!(ok);
        assert!(msg.is_none());
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

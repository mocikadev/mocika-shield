use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

fn no_window_command(prog: &str) -> Command {
    #[allow(unused_mut)]
    let mut cmd = Command::new(prog);
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

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

    pub fn from_str(s: &str) -> Self {
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

pub fn sign_apk(opts: &SignOptions) -> Result<()> {
    let apksigner = match &opts.apksigner_path {
        Some(p) if p.exists() => p.clone(),
        Some(p) => anyhow::bail!("配置的 apksigner.jar 路径不存在: {}", p.display()),
        None => find_apksigner()?,
    };

    let java = find_java()?;

    let apk_path_str = opts.apk_path.to_str().context("APK 路径包含非法字符")?;
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
    ];

    if v.v4 {
        cmd_args.push("--v4-signing-enabled");
        cmd_args.push("true");
    }

    let out_str;
    if let Some(out) = &opts.output_path {
        out_str = out.to_str().context("输出路径包含非法字符")?.to_string();
        cmd_args.push("--out");
        cmd_args.push(&out_str);
    }

    cmd_args.push(apk_path_str);

    let output = no_window_command(&java)
        .args(&cmd_args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .context("启动 apksigner 失败，请确认 Java 可用")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let code = output.status.code().unwrap_or(-1);
        anyhow::bail!("apksigner 签名失败（退出码 {}）: {}", code, stderr.trim());
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

fn find_java() -> Result<String> {
    #[cfg(target_os = "windows")]
    {
        if let Ok(java_home) = std::env::var("JAVA_HOME") {
            let java_exe = PathBuf::from(&java_home).join("bin").join("java.exe");
            if java_exe.exists() {
                return Ok(java_exe.to_str().unwrap_or("java").to_string());
            }
        }
    }
    Ok("java".to_string())
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keystore_type_from_str_jks_is_default() {
        assert_eq!(KeystoreType::from_str("JKS"), KeystoreType::Jks);
        assert_eq!(KeystoreType::from_str("jks"), KeystoreType::Jks);
        assert_eq!(KeystoreType::from_str("unknown"), KeystoreType::Jks);
    }

    #[test]
    fn keystore_type_from_str_pkcs12_variants() {
        assert_eq!(KeystoreType::from_str("PKCS12"), KeystoreType::Pkcs12);
        assert_eq!(KeystoreType::from_str("pkcs12"), KeystoreType::Pkcs12);
        assert_eq!(KeystoreType::from_str("P12"), KeystoreType::Pkcs12);
        assert_eq!(KeystoreType::from_str("p12"), KeystoreType::Pkcs12);
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
}

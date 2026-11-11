use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

mod diagnostic;
mod runner;
use diagnostic::classify_apksigner_error;
pub(crate) use diagnostic::fixed_failure;
#[cfg(any(windows, test))]
pub(crate) use runner::run_default as sign_apk_default;

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
    on_progress: impl FnMut(SigningProgressStep) -> std::result::Result<(), String>,
) -> Result<()> {
    #[cfg(windows)]
    return sign_apk_default(opts, on_progress);
    #[cfg(not(windows))]
    runner::run(opts, None, on_progress)
}

/// 显式适配入口：Java 及 opts.apksigner_path 必须已选定，不重新搜索全局工具。
/// 借用工作根及其材料由调用方确认停止后回收；不承诺同步硬超时或整组停止。
/// 保留普通签名的 V4 与输出覆盖语义，受控任务仍由外层限制为 V1～V3。
/// 原地签名要求借用工作根与输入同卷；不在跨卷失败时删除原包或自动改选工作根。
pub fn sign_apk_with_context(
    context: &crate::JavaToolContext,
    java: &Path,
    opts: &SignOptions,
    on_progress: impl FnMut(SigningProgressStep) -> std::result::Result<(), String>,
) -> Result<()> {
    runner::run(opts, Some((context, java)), on_progress)
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
}

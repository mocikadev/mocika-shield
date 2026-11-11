//! 核心 Java 语义路由：默认沿原工具行为，受控 Windows 仅使用绑定适配上下文。
use shield_core::{JavaToolContext, PreflightOptions, PreflightReport};
use std::path::Path;

pub(super) struct Tools<'a> {
    pub java: &'a Path,
    context: Option<&'a JavaToolContext>,
}
impl<'a> Tools<'a> {
    pub fn new(java: &'a Path, context: Option<&'a JavaToolContext>) -> Self {
        Self { java, context }
    }
    pub fn certificate(
        &self,
        path: &Path,
        alias: &str,
        password: &str,
        kind: &str,
    ) -> anyhow::Result<String> {
        match self.context {
            Some(context) => shield_core::keytool::extract_keystore_cert_fingerprint_with_context(
                context,
                self.java,
                path,
                alias,
                password,
                Some(kind),
            ),
            None => {
                shield_core::extract_keystore_cert_fingerprint(path, alias, password, Some(kind))
            }
        }
    }
    #[cfg(all(
        feature = "agent-executor",
        any(target_os = "macos", target_os = "linux", windows)
    ))]
    pub fn inspect(
        &self,
        path: &Path,
        signer: &Path,
    ) -> anyhow::Result<shield_core::ApkCheckOutcome> {
        match self.context {
            Some(context) => shield_core::check_apk_with_context(context, self.java, path, signer),
            None => shield_core::check_apk(path, Some(signer)),
        }
    }
    #[cfg(all(
        feature = "agent-executor",
        any(target_os = "macos", target_os = "linux", windows)
    ))]
    pub fn fingerprint(&self, path: &Path, signer: &Path) -> anyhow::Result<String> {
        match self.context {
            Some(context) => shield_core::extract_apk_cert_fingerprint_with_context(
                context, self.java, path, signer,
            ),
            None => shield_core::extract_apk_cert_fingerprint(path, Some(signer)),
        }
    }
    pub fn preflight(
        &self,
        path: &Path,
        options: PreflightOptions<'_>,
    ) -> anyhow::Result<PreflightReport> {
        match self.context {
            Some(context) => {
                shield_core::preflight_apk_with_context(context, self.java, path, options)
            }
            None => shield_core::preflight_apk(path, options),
        }
    }
    #[cfg(all(
        feature = "agent-executor",
        any(target_os = "macos", target_os = "linux", windows)
    ))]
    pub fn protect(
        &self,
        options: &shield_core::ProtectOptions,
        progress: impl Fn(shield_core::ProgressEvent) + Send + 'static,
        cancel: std::sync::Arc<std::sync::atomic::AtomicBool>,
    ) -> Result<(), shield_core::ShieldError> {
        match self.context {
            Some(context) => {
                shield_core::protect_apk_with_context(context, self.java, options, progress, cancel)
            }
            None => shield_core::protect_apk(options, progress, cancel),
        }
    }
    #[cfg(all(
        feature = "agent-executor",
        any(target_os = "macos", target_os = "linux", windows)
    ))]
    pub fn sign(
        &self,
        options: &shield_core::SignOptions,
        progress: impl FnMut(shield_core::SigningProgressStep) -> Result<(), String>,
    ) -> anyhow::Result<()> {
        match self.context {
            Some(context) => {
                shield_core::sign_apk_with_context(context, self.java, options, progress)
            }
            None => shield_core::sign_apk_with_progress(options, progress),
        }
    }
}

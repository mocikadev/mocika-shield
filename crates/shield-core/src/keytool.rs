//! 证书工具语义与旧调用兼容入口；不拥有资料库或最终证书发布责任。
use crate::JavaToolContext;
use anyhow::Result;
use std::ffi::OsStr;
use std::path::Path;
use std::process::Command;
mod creation;
#[cfg(any(not(windows), test))]
mod generation;
#[cfg(any(not(windows), test))]
mod legacy;
#[cfg(test)]
mod ordinary_tests;
pub use creation::{
    create_keystore_draft, create_keystore_draft_with_context, generate_keystore_draft,
    CreateKeystoreOptions, CreatedKeystore,
};
#[cfg(test)]
pub(crate) use legacy::parse_sha256_from_keytool;

/// Windows 普通证书查询使用匹配适配资源；其他平台保留原工具行为。
pub fn extract_keystore_cert_fingerprint(
    path: &Path,
    alias: &str,
    password: &str,
    kind: Option<&str>,
) -> Result<String> {
    #[cfg(windows)]
    return crate::java_adapter::certificate_session(|context, java| {
        context.certificate_fingerprint(java, path, alias, password, kind)
    });
    #[cfg(not(windows))]
    legacy::fingerprint(path, alias, password, kind)
}

/// Windows Alias 不经过原生参数或本地编码文本；其他平台保留原工具行为。
pub fn query_keystore_aliases(
    path: &Path,
    password: &str,
    kind: Option<&str>,
) -> Result<Vec<String>> {
    #[cfg(windows)]
    return crate::java_adapter::certificate_session(|context, java| {
        context.certificate_aliases(java, path, password, kind)
    });
    #[cfg(not(windows))]
    legacy::aliases(path, password, kind)
}

/// 使用显式 Java、适配资源及私有工作根进行只读查询；密码不进入请求文件。
pub fn query_keystore_aliases_with_context(
    context: &JavaToolContext,
    java: &Path,
    path: &Path,
    password: &str,
    kind: Option<&str>,
) -> Result<Vec<String>> {
    context.certificate_aliases(java, path, password, kind)
}

/// 只读取所选条目的证书 SHA-256，不恢复私钥、不检查或修改 Key 密码。
pub fn extract_keystore_cert_fingerprint_with_context(
    context: &JavaToolContext,
    java: &Path,
    path: &Path,
    alias: &str,
    password: &str,
    kind: Option<&str>,
) -> Result<String> {
    context.certificate_fingerprint(java, path, alias, password, kind)
}

/// 保留 Windows 隐藏控制台行为，仅配置本次子进程，不修改系统 Java 环境。
pub fn keytool_command(path: impl AsRef<OsStr>) -> Command {
    crate::java_process::keytool_command(path)
}

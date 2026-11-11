//! 新建证书的最小语义输入；不携带显示偏好、默认项或入库策略。
use crate::JavaToolContext;
use anyhow::Result;
use std::path::Path;

/// 仅供本次本地执行，不实现 Debug 或序列化，密码不进入请求文件。
pub struct CreateKeystoreOptions<'a> {
    pub kind: &'a str,
    pub alias: &'a str,
    pub keystore_password: &'a str,
    /// 留空时使用 Store 密码；不改变调用方保存的偏好。
    pub key_password: &'a str,
    pub dname: &'a str,
    pub validity_days: u32,
    pub key_size: u32,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreatedKeystore {
    pub alias: String,
    pub fingerprint: String,
}

/// 兼容既有返回值；Windows 使用默认适配，其余平台保留原新建工具。
pub fn generate_keystore_draft(output: &Path, options: &CreateKeystoreOptions<'_>) -> Result<()> {
    #[cfg(windows)]
    return create_keystore_draft(output, options).map(|_| ());
    #[cfg(not(windows))]
    super::generation::generate(output, options)
}

/// 普通同步适配入口，自动选择同包资源及本次 Java；结果包含核验后的实际 Alias。
/// 目标必须是调用方新建的私有暂存目录，最终发布及资料保存仍由调用方负责。
pub fn create_keystore_draft(
    output: &Path,
    options: &CreateKeystoreOptions<'_>,
) -> Result<CreatedKeystore> {
    crate::java_adapter::certificate_session(|context, java| {
        let keytool = crate::java_tools::find_keytool(Some(java)).ok_or_else(|| {
            crate::diagnostic::Diagnostic::new(crate::diagnostic::FailureCode::ToolNotFound).attach(
                anyhow::anyhow!("未找到可执行的 keytool；新建证书需要完整 JDK 8+"),
            )
        })?;
        create_keystore_draft_with_context(context, java, &keytool, output, options)
    })
}

/// 只在新的私有暂存目标生成证书；调用方拥有工作根、目标清理与最终发布责任。
/// 失败可能保留本次私有材料，不代表整组停止，不能自动重试或提交。
pub fn create_keystore_draft_with_context(
    context: &JavaToolContext,
    java: &Path,
    keytool: &Path,
    output: &Path,
    options: &CreateKeystoreOptions<'_>,
) -> Result<CreatedKeystore> {
    context.create_certificate(java, keytool, output, options)
}

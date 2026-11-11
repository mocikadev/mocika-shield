//! 共享证书只读用例的错误适配；工具选择、参数及解析统一由核心库负责。
use super::CertificateError;
use shield_core::diagnostic::Diagnostic;
use std::path::Path;

pub fn query_keystore_aliases(
    keystore_path: String,
    ks_pass: String,
    ks_type: Option<String>,
) -> Result<Vec<String>, CertificateError> {
    shield_core::keytool::query_keystore_aliases(
        Path::new(&keystore_path),
        &ks_pass,
        ks_type.as_deref(),
    )
    .map_err(|error| CertificateError::diagnosed(error.to_string(), Diagnostic::from_error(&error)))
}

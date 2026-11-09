//! 证书用例的本机错误；不携带界面框架或可序列化的秘密材料。
use shield_core::diagnostic::{Diagnostic, FailureCode};
pub struct CertificateError {
    pub message: String,
    pub diagnostic: Box<Diagnostic>,
    pub executed: bool,
}
impl CertificateError {
    pub fn diagnosed(message: String, diagnostic: Diagnostic) -> Self {
        Self {
            message,
            diagnostic: Box::new(diagnostic),
            executed: true,
        }
    }
    pub fn preflight(message: String, diagnostic: Diagnostic) -> Self {
        Self {
            message,
            diagnostic: Box::new(diagnostic),
            executed: false,
        }
    }
}
impl From<String> for CertificateError {
    fn from(message: String) -> Self {
        Self::diagnosed(message, Diagnostic::new(FailureCode::Unknown))
    }
}
impl std::fmt::Display for CertificateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.message.fmt(f)
    }
}
impl std::fmt::Debug for CertificateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CertificateError")
            .field("code", &self.diagnostic.code)
            .field("executed", &self.executed)
            .finish_non_exhaustive()
    }
}
impl std::error::Error for CertificateError {}

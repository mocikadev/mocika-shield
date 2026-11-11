//! 共享证书用例、存储和安全模型；不依赖入口框架。
mod creation;
#[cfg(test)]
mod creation_tests;
mod crypto;
mod error;
mod keytool;
mod managed;
mod models;
mod service;
mod store;
pub use creation::{create_managed_certificate, create_managed_certificate_with_context};
pub use error::CertificateError;
pub use keytool::query_keystore_aliases;
pub use models::*;
pub use service::{
    resolve_signing_certificate, save_certificate_profile, validate_certificate_input,
    verify_saved_certificate, CertificateOutcome,
};
pub use store::{
    BoundSigningIdentity, CertificateBinding, CertificateBindingError, CertificateStoreState,
};

pub fn normalize_keystore_type(value: Option<&str>) -> Option<String> {
    match value.map(|s| s.trim()).filter(|s| !s.is_empty()) {
        Some(raw) if raw.eq_ignore_ascii_case("pkcs12") || raw.eq_ignore_ascii_case("p12") => {
            Some("PKCS12".to_string())
        }
        _ => Some("JKS".to_string()),
    }
}

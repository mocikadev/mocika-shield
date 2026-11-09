//! GUI 只负责从 Tauri 定位目录，证书库与密文逻辑由共享应用层维护。
pub(crate) use shield_app::certificates::{
    CertificateRecord, CertificateStoreState, CertificateSummary, CertificateUpsertInput,
    CertificateValidationInput, CertificateValidationResult, CreateManagedCertificateInput,
};

pub(crate) fn initialize_certificate_store(
    app: &tauri::AppHandle,
) -> Result<CertificateStoreState, String> {
    let paths = crate::app_paths::application_paths(app)?;
    CertificateStoreState::initialize(paths.certificate_database(), paths.keystore_dir())
}

#[cfg(test)]
#[path = "cert_unicode_tests.rs"]
mod unicode_tests;

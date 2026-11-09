//! GUI 证书命令复用共享用例；诊断上报仍由 GUI 完成。
pub(crate) use shield_app::certificates::{
    create_managed_certificate, save_certificate_profile, validate_certificate_input,
    verify_saved_certificate, CertificateOutcome,
};

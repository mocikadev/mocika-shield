pub mod apk_inspect;
mod dex_packer;
#[cfg(test)]
mod dex_research;
pub mod error;
mod preflight;
mod protect;
mod protect_api;
pub mod signing;
pub mod utils;
pub mod zipalign;

pub use apk_inspect::{
    check_apk, extract_apk_cert_fingerprint, extract_keystore_cert_fingerprint,
    normalize_fingerprint, ApkCheckOutcome,
};
pub use error::ShieldError;
pub use preflight::{
    preflight_apk, PreflightCheck, PreflightFacts, PreflightOptions, PreflightReport,
    PreflightSeverity, RuntimeProfile,
};
pub use protect_api::{
    protect_apk, EnvironmentPolicy, ProgressEvent, ProgressStep, ProtectOptions,
};
pub use signing::{
    check_apksigner, find_apksigner, sign_apk, sign_apk_with_progress, KeystoreType, SignOptions,
    SigningProgressStep, SigningVersions,
};
pub use zipalign::{align_apk, verify_apk_alignment, AlignmentIssue};

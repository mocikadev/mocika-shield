pub mod apk_inspect;
mod dex_packer;
pub mod error;
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
pub use protect_api::{protect_apk, ProgressEvent, ProgressStep, ProtectOptions};
pub use signing::{
    check_apksigner, find_apksigner, sign_apk, sign_apk_with_progress, KeystoreType, SignOptions,
    SigningProgressStep, SigningVersions,
};
pub use zipalign::{align_apk, verify_apk_alignment, AlignmentIssue};

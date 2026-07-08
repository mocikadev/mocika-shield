mod dex_packer;
pub mod error;
mod protect;
mod protect_api;
pub mod signing;
pub mod utils;
pub mod zipalign;

pub use error::ShieldError;
pub use protect_api::{protect_apk, ProgressEvent, ProgressStep, ProtectOptions};
pub use signing::{
    check_apksigner, find_apksigner, sign_apk, KeystoreType, SignOptions, SigningVersions,
};
pub use zipalign::{align_apk, verify_apk_alignment, AlignmentIssue};

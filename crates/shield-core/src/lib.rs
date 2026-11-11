pub mod apk_inspect;
mod bundle_resources;
mod dex_classes;
mod dex_packer;
#[cfg(test)]
mod dex_research;
pub mod diagnostic;
pub mod error;
mod java_adapter;
#[cfg(windows)]
pub mod java_directory;
mod java_process;
mod java_tools;
pub mod keytool;
mod mshd;
mod preflight;
mod protect;
mod protect_api;
mod runtime_resources;
pub mod signing;
mod signing_input;
mod tool_evidence;
pub mod utils;
#[cfg(windows)]
pub mod windows_private;
pub mod zipalign;

pub use apk_inspect::{
    check_apk, check_apk_with_context, extract_apk_cert_fingerprint,
    extract_apk_cert_fingerprint_with_context, extract_keystore_cert_fingerprint,
    normalize_fingerprint, ApkCheckOutcome,
};
pub use error::ShieldError;
pub use java_adapter::{find_java_adapter, ApktoolProject, JavaToolContext};
pub use preflight::{
    preflight_apk, preflight_apk_with_context, PreflightCheck, PreflightFacts, PreflightOptions,
    PreflightReport, PreflightSeverity, RuntimeProfile,
};
pub use protect_api::{
    protect_apk, protect_apk_with_context, EnvironmentPolicy, ProgressEvent, ProgressStep,
    ProtectOptions,
};
pub use runtime_resources::validate_runtime_resources;
pub use signing::{
    check_apksigner, find_apksigner, sign_apk, sign_apk_with_context, sign_apk_with_progress,
    KeystoreType, SignOptions, SigningProgressStep, SigningVersions,
};
pub use signing_input::{inspect_signing_input, SigningInput};
pub use zipalign::{align_apk, verify_apk_alignment, AlignmentIssue};

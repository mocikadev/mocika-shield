pub mod apk_inspect;
mod dex_packer;
pub mod error;
mod protect;
mod protect_api;
pub mod signing;
pub mod utils;
pub mod zipalign;

#[cfg(feature = "aab-experiment")]
#[doc(hidden)]
pub mod aab_experiment {
    use std::path::Path;

    /// 仅供 AAB 探索分支复用正式 DEXB v5 packer，不属于稳定公共接口。
    pub fn pack_dex_files(
        input_dir: &Path,
        output_bin: &Path,
        ikm: &[u8],
        signature: &str,
    ) -> anyhow::Result<()> {
        crate::dex_packer::pack_dex_files(input_dir, output_bin, ikm, signature)
    }
}

pub use apk_inspect::{
    check_apk, extract_apk_cert_fingerprint, extract_keystore_cert_fingerprint,
    normalize_fingerprint, ApkCheckOutcome,
};
pub use error::ShieldError;
pub use protect_api::{protect_apk, ProgressEvent, ProgressStep, ProtectOptions};
pub use signing::{
    check_apksigner, find_apksigner, sign_apk, KeystoreType, SignOptions, SigningVersions,
};
pub use zipalign::{align_apk, verify_apk_alignment, AlignmentIssue};

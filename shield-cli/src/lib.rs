pub mod commands;
mod dex_packer;
pub mod error;
pub mod utils;

pub use commands::protect::{protect_apk, ProgressEvent, ProgressStep, ProtectOptions};
pub use commands::sign::{
    check_apksigner, find_apksigner, sign_apk, KeystoreType, SignOptions, SigningVersions,
};
pub use error::ShieldError;

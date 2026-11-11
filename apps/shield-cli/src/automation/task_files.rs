//! 任务文件的原生边界；只管理私有文件、目录身份及无覆盖提交，不拥有任务终态。
#[cfg(unix)]
#[path = "task_files/unix.rs"]
mod platform;

#[cfg(any(windows, test))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct FileIdentity {
    volume: u64,
    object: [u8; 16],
}

#[cfg(windows)]
#[derive(Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct FileStamp {
    identity: FileIdentity,
    pub(super) bytes: u64,
    attributes: u32,
    created: i64,
    modified: i64,
    changed: i64,
}

#[cfg(windows)]
pub(super) use platform::file_stamp;
#[cfg(all(windows, test))]
pub(super) use platform::same_directory;

#[cfg(all(unix, test))]
pub(super) use platform::file_identity;
#[cfg(all(windows, any(test, feature = "mcp-server")))]
pub(crate) use platform::file_identity;
#[cfg(all(windows, feature = "mcp-server"))]
pub(crate) use platform::stamp_file;
#[cfg(all(windows, test))]
pub(super) use platform::test_make_public;
#[cfg(windows)]
pub(crate) use platform::{inspect_directory, regular, same_entry, validate_name};
#[cfg(unix)]
pub(super) use platform::{inspect_directory, regular, same_entry, validate_name};
#[cfg(windows)]
#[path = "task_files/windows.rs"]
mod platform;

pub(super) use platform::{
    artifact, commit_no_replace, complete_materials, create_at, open_directory, private_directory,
    sync_directory, validate_directory, validate_private_file, WorkDirectory,
};

pub(super) fn resolved(path: &std::path::Path) -> std::io::Result<std::path::PathBuf> {
    std::fs::canonicalize(path).map(|path| shield_core::utils::strip_unc_prefix(&path))
}

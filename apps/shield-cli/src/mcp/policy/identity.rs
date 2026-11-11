//! MCP 可信文件的身份印记；Unix 字段保持，Windows 借用已验收的原生文件边界。
use super::Result;
use std::fs::{File, Metadata};
#[cfg(unix)]
use std::os::unix::fs::MetadataExt;

#[cfg(unix)]
#[derive(Clone, PartialEq, Eq)]
pub(super) struct Identity {
    dev: u64,
    ino: u64,
}
#[cfg(windows)]
pub(super) type Identity = crate::automation::task_files::FileIdentity;

#[cfg(unix)]
pub(super) fn identity(_file: &File, meta: &Metadata) -> Result<Identity> {
    Ok(Identity {
        dev: meta.dev(),
        ino: meta.ino(),
    })
}
#[cfg(windows)]
pub(super) fn identity(file: &File, _meta: &Metadata) -> Result<Identity> {
    crate::automation::task_files::file_identity(file).map_err(|_| "TRUSTED_FILE_INVALID")
}

#[cfg(unix)]
type Stamp = (u64, u64, u64, i64, i64, i64, i64);
#[cfg(windows)]
type Stamp = crate::automation::task_files::FileStamp;

#[cfg(unix)]
pub(super) fn stamp(_file: &File, meta: &Metadata) -> Result<Stamp> {
    Ok((
        meta.dev(),
        meta.ino(),
        meta.len(),
        meta.mtime(),
        meta.mtime_nsec(),
        meta.ctime(),
        meta.ctime_nsec(),
    ))
}
#[cfg(windows)]
pub(super) fn stamp(file: &File, _meta: &Metadata) -> Result<Stamp> {
    crate::automation::task_files::stamp_file(file).map_err(|_| "TRUSTED_FILE_CHANGED")
}

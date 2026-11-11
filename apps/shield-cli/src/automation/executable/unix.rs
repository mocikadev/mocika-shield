//! Unix 执行身份印记，保留原字段与权限检查；不拥有任务或凭据。
use super::{error, CheckError};
use serde::{Deserialize, Serialize};
use std::{os::unix::fs::MetadataExt, path::Path};

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(in crate::automation) struct Stamp {
    device: u64,
    inode: u64,
    mode: u32,
    pub(super) bytes: u64,
    modified: i64,
    modified_nanos: i64,
    changed: i64,
    changed_nanos: i64,
}
pub(super) fn read(path: &Path) -> Result<Stamp, CheckError> {
    let stamp = read_regular(path)?;
    if stamp.mode & 0o111 == 0 {
        return Err(error("EXECUTABLE_CHANGED"));
    }
    Ok(stamp)
}
pub(super) fn read_regular(path: &Path) -> Result<Stamp, CheckError> {
    let metadata = std::fs::metadata(path).map_err(|_| error("EXECUTABLE_CHANGED"))?;
    if !metadata.is_file() {
        return Err(error("EXECUTABLE_CHANGED"));
    }
    Ok(Stamp {
        device: metadata.dev(),
        inode: metadata.ino(),
        mode: metadata.mode(),
        bytes: metadata.len(),
        modified: metadata.mtime(),
        modified_nanos: metadata.mtime_nsec(),
        changed: metadata.ctime(),
        changed_nanos: metadata.ctime_nsec(),
    })
}

//! 规划期间的文件身份与输出位置检查；不占位、不覆盖，不替代执行时的原子写入。
use super::{require_file, CheckError};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use shield_core::utils::strip_unc_prefix;
use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(super) struct FileSnapshot {
    pub role: String,
    pub lookup_path: PathBuf,
    pub path: PathBuf,
    pub size: u64,
    pub sha256: String,
}

pub(super) fn file(role: &str, path: &Path) -> Result<FileSnapshot, CheckError> {
    file_checked(role, path, &mut || Ok(()))
}

pub(super) fn file_checked(
    role: &str,
    path: &Path,
    checkpoint: &mut impl FnMut() -> Result<(), CheckError>,
) -> Result<FileSnapshot, CheckError> {
    checkpoint()?;
    require_file(path, "SOURCE_NOT_FILE")?;
    let lookup_path = if path.is_absolute() {
        strip_unc_prefix(path)
    } else {
        std::env::current_dir()
            .map_err(|e| CheckError::from_core(e.into()))?
            .join(path)
    };
    let path = strip_unc_prefix(
        &fs::canonicalize(&lookup_path).map_err(|e| CheckError::from_core(e.into()))?,
    );
    if path.to_str().is_none() || lookup_path.to_str().is_none() {
        return Err(CheckError::new(
            "PATH_ENCODING_UNSUPPORTED",
            "文件路径无法安全纳入自动化计划。",
        ));
    }
    let mut input = File::open(&path).map_err(|e| CheckError::from_core(e.into()))?;
    let mut hash = Sha256::new();
    let mut size = 0u64;
    let mut buffer = [0u8; 64 * 1024];
    loop {
        checkpoint()?;
        let count = input
            .read(&mut buffer)
            .map_err(|e| CheckError::from_core(e.into()))?;
        if count == 0 {
            break;
        }
        hash.update(&buffer[..count]);
        size += count as u64;
    }
    Ok(FileSnapshot {
        role: role.to_owned(),
        lookup_path,
        path,
        size,
        sha256: format!("{:x}", hash.finalize()),
    })
}

pub(super) fn recheck(snapshots: &[&FileSnapshot]) -> Result<(), CheckError> {
    recheck_checked(snapshots, &mut || Ok(()))
}

pub(super) fn recheck_checked(
    snapshots: &[&FileSnapshot],
    checkpoint: &mut impl FnMut() -> Result<(), CheckError>,
) -> Result<(), CheckError> {
    for &before in snapshots {
        let after =
            file_checked(&before.role, &before.lookup_path, checkpoint).map_err(|error| {
                if matches!(error.code, "TASK_TIMEOUT" | "TASK_CANCELLED") {
                    return error;
                }
                CheckError::new(
                    "PLAN_SOURCE_CHANGED",
                    "规划期间文件不可再读取，请重新检查。",
                )
            })?;
        if before != &after {
            return Err(CheckError::new(
                "PLAN_SOURCE_CHANGED",
                "规划期间文件身份发生变化，请重新检查；不会自动执行或重试。",
            ));
        }
    }
    Ok(())
}

pub(super) fn output(path: &Path) -> Result<PathBuf, CheckError> {
    let path = strip_unc_prefix(path);
    // symlink_metadata 同时识别悬空链接，不能使用 exists() 后再写的检查方式。
    match fs::symlink_metadata(&path) {
        Ok(_) => {
            return Err(CheckError::new(
                "OUTPUT_EXISTS",
                "计划输出已经存在，拒绝覆盖或沿链接写入。",
            ))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(CheckError::from_core(e.into())),
    }
    let name = path
        .file_name()
        .ok_or_else(|| CheckError::new("OUTPUT_PATH_INVALID", "请指定明确的输出文件名。"))?;
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let parent = fs::canonicalize(parent).map_err(|_| {
        CheckError::new(
            "OUTPUT_PARENT_UNAVAILABLE",
            "输出父目录不存在或不可访问，规划不会自动创建目录。",
        )
    })?;
    if !parent.is_dir() {
        return Err(CheckError::new(
            "OUTPUT_PARENT_UNAVAILABLE",
            "输出父位置不是目录。",
        ));
    }
    let resolved = strip_unc_prefix(&parent).join(name);
    if resolved.to_str().is_none() {
        return Err(CheckError::new(
            "PATH_ENCODING_UNSUPPORTED",
            "输出路径无法安全纳入自动化计划。",
        ));
    }
    // 父目录链接解析后再确认一次；这仍不是执行时的无覆盖保证。
    match fs::symlink_metadata(&resolved) {
        Ok(_) => Err(CheckError::new(
            "OUTPUT_EXISTS",
            "计划输出已经存在，拒绝覆盖或沿链接写入。",
        )),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(resolved),
        Err(e) => Err(CheckError::from_core(e.into())),
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;

    #[test]
    fn 材料链接改向即使内容相同也必须重新规划() {
        let root = tempfile::tempdir().unwrap();
        let first = root.path().join("first");
        let second = root.path().join("second");
        let link = root.path().join("selected");
        fs::write(&first, b"same bytes").unwrap();
        fs::write(&second, b"same bytes").unwrap();
        symlink(&first, &link).unwrap();
        let before = file("resources", &link).ok().unwrap();
        fs::remove_file(&link).unwrap();
        symlink(&second, &link).unwrap();
        let error = recheck(&[&before]).err().unwrap();
        assert_eq!(error.code, "PLAN_SOURCE_CHANGED");
    }

    #[test]
    fn 输出父链接改向产生不同的目标身份() {
        let root = tempfile::tempdir().unwrap();
        let first = root.path().join("first");
        let second = root.path().join("second");
        let link = root.path().join("destination");
        fs::create_dir(&first).unwrap();
        fs::create_dir(&second).unwrap();
        symlink(&first, &link).unwrap();
        let before = output(&link.join("output.apk")).ok().unwrap();
        fs::remove_file(&link).unwrap();
        symlink(&second, &link).unwrap();
        let after = output(&link.join("output.apk")).ok().unwrap();
        assert_ne!(before, after);
        assert!(!before.exists());
        assert!(!after.exists());
    }
}

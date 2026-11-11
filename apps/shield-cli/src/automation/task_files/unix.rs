//! Unix 任务文件：保留既有权限、目录句柄及原子硬链接语义。
use rustix::fs::{linkat, openat, AtFlags, Mode, OFlags, CWD};
use std::{
    ffi::OsStr,
    fs::{self, File},
    io,
    os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt},
    path::{Path, PathBuf},
};

pub(in crate::automation) struct WorkDirectory {
    directory: File,
    work: tempfile::TempDir,
}
impl WorkDirectory {
    pub(in crate::automation) fn new(path: &Path, parent: &File) -> io::Result<Self> {
        let work = tempfile::Builder::new()
            .prefix(".mocika-task-")
            .permissions(fs::Permissions::from_mode(0o700))
            .tempdir_in(path)?;
        let directory = File::from(openat(
            parent,
            work.path().file_name().ok_or(io::ErrorKind::InvalidInput)?,
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::empty(),
        )?);
        Ok(Self { directory, work })
    }
    pub(in crate::automation) fn path(&self) -> &Path {
        self.work.path()
    }
    pub(in crate::automation) fn directory(&self) -> &File {
        &self.directory
    }
    pub(in crate::automation) fn keep(self) -> PathBuf {
        self.work.keep()
    }
    pub(in crate::automation) fn close(self) -> io::Result<()> {
        drop(self.directory);
        self.work.close()
    }
}

pub(in crate::automation) fn validate_name(name: &OsStr) -> io::Result<()> {
    if name.is_empty() {
        return Err(io::ErrorKind::InvalidInput.into());
    }
    Ok(())
}
pub(in crate::automation) fn open_directory(path: &Path) -> io::Result<File> {
    openat(
        CWD,
        path,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .map(File::from)
    .map_err(Into::into)
}
pub(in crate::automation) use open_directory as inspect_directory;
pub(in crate::automation) fn private_directory(path: &Path) -> io::Result<()> {
    fs::DirBuilder::new().mode(0o700).create(path)
}
pub(in crate::automation) fn validate_directory(path: &Path) -> io::Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_dir()
        || metadata.permissions().mode() & 0o077 != 0
        || metadata.uid() != rustix::process::geteuid().as_raw()
    {
        return Err(io::ErrorKind::PermissionDenied.into());
    }
    Ok(())
}
pub(in crate::automation) fn regular(path: &Path) -> io::Result<File> {
    let file = File::from(openat(
        CWD,
        path,
        OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
        Mode::empty(),
    )?);
    if !file.metadata()?.is_file() {
        return Err(io::ErrorKind::InvalidData.into());
    }
    Ok(file)
}
pub(in crate::automation) fn create_at(
    directory: &File,
    _path: &Path,
    name: &OsStr,
) -> io::Result<File> {
    openat(
        directory,
        name,
        OFlags::RDWR | OFlags::CREATE | OFlags::EXCL | OFlags::CLOEXEC,
        Mode::RUSR | Mode::WUSR,
    )
    .map(File::from)
    .map_err(Into::into)
}
pub(in crate::automation) fn artifact(directory: &File, _path: &Path) -> io::Result<File> {
    let file = File::from(openat(
        directory,
        "signed.apk",
        OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
        Mode::empty(),
    )?);
    let info = file.metadata()?;
    if !info.is_file() || info.len() == 0 || info.nlink() != 1 {
        return Err(io::ErrorKind::InvalidData.into());
    }
    Ok(file)
}
pub(in crate::automation) fn validate_private_file(file: &File) -> io::Result<()> {
    let metadata = file.metadata()?;
    if metadata.nlink() != 1
        || metadata.permissions().mode() & 0o077 != 0
        || metadata.uid() != rustix::process::geteuid().as_raw()
    {
        return Err(io::ErrorKind::PermissionDenied.into());
    }
    Ok(())
}
pub(in crate::automation) fn same_entry(path: &Path, file: &File) -> bool {
    let (Ok(actual), Ok(expected)) = (fs::symlink_metadata(path), file.metadata()) else {
        return false;
    };
    actual.dev() == expected.dev() && actual.ino() == expected.ino()
}

#[cfg(test)]
pub(in crate::automation) fn file_identity(file: &File) -> io::Result<super::FileIdentity> {
    let metadata = file.metadata()?;
    let mut object = [0u8; 16];
    object[..8].copy_from_slice(&metadata.ino().to_le_bytes());
    Ok(super::FileIdentity {
        volume: metadata.dev(),
        object,
    })
}
pub(in crate::automation) fn commit_no_replace(
    directory: &File,
    source: &OsStr,
    _sealed: &File,
    parent: &File,
    target: &OsStr,
) -> io::Result<()> {
    linkat(directory, source, parent, target, AtFlags::empty()).map_err(Into::into)
}
pub(in crate::automation) fn complete_materials(directory: &File) -> io::Result<()> {
    directory.sync_all()
}
pub(in crate::automation) fn sync_directory(directory: &File) -> bool {
    directory.sync_all().is_ok()
}

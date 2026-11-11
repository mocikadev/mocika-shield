//! 私有引导材料的文件边界；只创建本次材料和只读核对，不启动进程或清理借用根。
use super::invalid;
use anyhow::Result;
#[cfg(unix)]
use std::fs::OpenOptions;
#[cfg(windows)]
use std::io;
use std::{
    fs::{self, File},
    path::{Path, PathBuf},
};

pub(super) fn directory(root: &Path) -> Result<PathBuf> {
    #[cfg(windows)]
    {
        for _ in 0..128 {
            let path = root.join(format!("java-tool-{:032x}", rand::random::<u128>()));
            match crate::windows_private::create_directory(&path) {
                Ok(()) => {
                    validate_directory(&path)?;
                    return Ok(path);
                }
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error.into()),
            }
        }
        Err(invalid())
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        Ok(tempfile::Builder::new()
            .prefix("java-tool-")
            .permissions(fs::Permissions::from_mode(0o700))
            .tempdir_in(root)?
            .keep())
    }
}

pub(super) fn create_file(path: &Path) -> Result<File> {
    #[cfg(windows)]
    {
        Ok(crate::windows_private::create_file(path)?)
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        Ok(OpenOptions::new()
            .create_new(true)
            .write(true)
            .mode(0o600)
            .open(path)?)
    }
}

pub(super) fn validate_directory(path: &Path) -> Result<()> {
    #[cfg(windows)]
    {
        let file = crate::java_directory::open_directory(path)?;
        crate::windows_private::validate(&file, true)?;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let metadata = fs::symlink_metadata(path)?;
        use std::os::unix::fs::MetadataExt;
        if !metadata.is_dir()
            || metadata.permissions().mode() & 0o077 != 0
            || metadata.uid() != unsafe { libc::geteuid() }
        {
            return Err(invalid());
        }
    }
    Ok(())
}

pub(super) fn identity(file: &File) -> Result<(u64, [u8; 16])> {
    #[cfg(windows)]
    {
        Ok(crate::java_directory::identity(file)?)
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let metadata = file.metadata()?;
        Ok((metadata.dev(), (metadata.ino() as u128).to_le_bytes()))
    }
}

pub(super) fn readonly(path: &Path) -> Result<File> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_file() {
        return Err(invalid());
    }
    #[cfg(windows)]
    let file = crate::windows_private::readonly(path)?;
    #[cfg(unix)]
    let file = {
        use std::os::unix::fs::OpenOptionsExt;
        // 普通文件不受非阻塞标志影响；若路径被换成 FIFO，也不能在校验前阻塞。
        OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC)
            .open(path)?
    };
    validate_file(&file)?;
    Ok(file)
}

pub(super) fn read_back(path: &Path, expected: (u64, [u8; 16]), bytes: usize) -> Result<File> {
    let file = readonly(path)?;
    if file.metadata()?.len() != bytes as u64 || identity(&file)? != expected {
        return Err(invalid());
    }
    Ok(file)
}

pub(super) fn validate_file(file: &File) -> Result<()> {
    if !file.metadata()?.is_file() {
        return Err(invalid());
    }
    #[cfg(windows)]
    {
        use std::{mem::size_of, os::windows::io::AsRawHandle};
        use windows_sys::Win32::Storage::FileSystem::*;
        let mut tag = FILE_ATTRIBUTE_TAG_INFO::default();
        if unsafe {
            GetFileInformationByHandleEx(
                file.as_raw_handle(),
                FileAttributeTagInfo,
                (&mut tag as *mut FILE_ATTRIBUTE_TAG_INFO).cast(),
                size_of::<FILE_ATTRIBUTE_TAG_INFO>() as u32,
            )
        } == 0
        {
            return Err(io::Error::last_os_error().into());
        }
        if unsafe { GetFileType(file.as_raw_handle()) } != FILE_TYPE_DISK
            || tag.FileAttributes & (FILE_ATTRIBUTE_REPARSE_POINT | FILE_ATTRIBUTE_DIRECTORY) != 0
        {
            return Err(invalid());
        }
        let mut info = FILE_STANDARD_INFO::default();
        if unsafe {
            GetFileInformationByHandleEx(
                file.as_raw_handle(),
                FileStandardInfo,
                (&mut info as *mut FILE_STANDARD_INFO).cast(),
                size_of::<FILE_STANDARD_INFO>() as u32,
            )
        } == 0
        {
            return Err(io::Error::last_os_error().into());
        }
        if info.NumberOfLinks != 1 || info.DeletePending {
            return Err(invalid());
        }
        crate::windows_private::validate(file, false)?;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        let metadata = file.metadata()?;
        if metadata.nlink() != 1
            || metadata.permissions().mode() & 0o077 != 0
            || metadata.uid() != unsafe { libc::geteuid() }
        {
            return Err(invalid());
        }
    }
    Ok(())
}

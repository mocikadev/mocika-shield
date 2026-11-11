//! Windows 文件身份与不跟随打开；只使用已拥有的文件句柄，不伪造 Unix inode。
use std::{
    ffi::OsStr,
    fs::File,
    io,
    mem::size_of,
    os::windows::{
        ffi::OsStrExt,
        io::{AsRawHandle, FromRawHandle, OwnedHandle},
    },
    path::Path,
};
use windows_sys::Win32::{Foundation::INVALID_HANDLE_VALUE, Storage::FileSystem::*};

pub(super) fn wide(value: &OsStr) -> io::Result<Vec<u16>> {
    let units: Vec<_> = value.encode_wide().collect();
    if units.is_empty() || units.contains(&0) {
        return Err(io::ErrorKind::InvalidInput.into());
    }
    Ok(units.into_iter().chain(Some(0)).collect())
}

pub(super) fn path_wide(path: &Path) -> io::Result<Vec<u16>> {
    // 使用系统返回的原生父路径支持长路径，不手写 UNC 前缀；最后一项不解析链接。
    let absolute = std::path::absolute(path)?;
    let path = if let Some(name) = absolute.file_name() {
        std::fs::canonicalize(absolute.parent().ok_or(io::ErrorKind::InvalidInput)?)?.join(name)
    } else {
        std::fs::canonicalize(absolute)?
    };
    wide(path.as_os_str())
}

pub(super) fn open(path: &Path, access: u32, share: u32, directory: bool) -> io::Result<File> {
    let name = path_wide(path)?;
    // 安全：路径在调用期间存活；不跟随最后一项的重解析点，句柄不可继承。
    let raw = unsafe {
        CreateFileW(
            name.as_ptr(),
            access,
            share,
            std::ptr::null(),
            OPEN_EXISTING,
            FILE_FLAG_OPEN_REPARSE_POINT
                | if directory {
                    FILE_FLAG_BACKUP_SEMANTICS
                } else {
                    FILE_ATTRIBUTE_NORMAL
                },
            std::ptr::null_mut(),
        )
    };
    if raw == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error());
    }
    let file = File::from(unsafe { OwnedHandle::from_raw_handle(raw) });
    kind(&file, directory)?;
    Ok(file)
}

pub(super) fn kind(file: &File, directory: bool) -> io::Result<()> {
    let mut info = FILE_ATTRIBUTE_TAG_INFO::default();
    if unsafe {
        GetFileInformationByHandleEx(
            file.as_raw_handle(),
            FileAttributeTagInfo,
            (&mut info as *mut FILE_ATTRIBUTE_TAG_INFO).cast(),
            size_of::<FILE_ATTRIBUTE_TAG_INFO>() as u32,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    if unsafe { GetFileType(file.as_raw_handle()) } != FILE_TYPE_DISK
        || info.FileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0
        || (info.FileAttributes & FILE_ATTRIBUTE_DIRECTORY != 0) != directory
    {
        return Err(io::ErrorKind::InvalidData.into());
    }
    Ok(())
}

pub(super) fn identity(file: &File) -> io::Result<crate::automation::task_files::FileIdentity> {
    let mut info = FILE_ID_INFO::default();
    if unsafe {
        GetFileInformationByHandleEx(
            file.as_raw_handle(),
            FileIdInfo,
            (&mut info as *mut FILE_ID_INFO).cast(),
            size_of::<FILE_ID_INFO>() as u32,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    if info.FileId.Identifier == [0; 16] {
        return Err(io::ErrorKind::Unsupported.into());
    }
    Ok(crate::automation::task_files::FileIdentity {
        volume: info.VolumeSerialNumber,
        object: info.FileId.Identifier,
    })
}
pub(super) fn single_link(file: &File) -> io::Result<()> {
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
        return Err(io::Error::last_os_error());
    }
    if info.NumberOfLinks != 1 || info.DeletePending {
        return Err(io::ErrorKind::InvalidData.into());
    }
    Ok(())
}

pub(super) fn stamp(path: &Path) -> io::Result<crate::automation::task_files::FileStamp> {
    let file = open(
        path,
        FILE_READ_ATTRIBUTES | READ_CONTROL,
        FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
        false,
    )?;
    stamp_file(&file)
}

pub(super) fn stamp_file(file: &File) -> io::Result<crate::automation::task_files::FileStamp> {
    let first = basic(file)?;
    let identity = identity(file)?;
    let bytes = file.metadata()?.len();
    let after = basic(file)?;
    if first.CreationTime != after.CreationTime
        || first.LastWriteTime != after.LastWriteTime
        || first.ChangeTime != after.ChangeTime
        || first.FileAttributes != after.FileAttributes
    {
        return Err(io::ErrorKind::WouldBlock.into());
    }
    Ok(crate::automation::task_files::FileStamp {
        identity,
        bytes,
        attributes: after.FileAttributes,
        created: after.CreationTime,
        modified: after.LastWriteTime,
        changed: after.ChangeTime,
    })
}

fn basic(file: &File) -> io::Result<FILE_BASIC_INFO> {
    let mut info = FILE_BASIC_INFO::default();
    // 安全：使用 SDK 完整类型和本次借用句柄，读取过程中不按路径重新打开对象。
    if unsafe {
        GetFileInformationByHandleEx(
            file.as_raw_handle(),
            FileBasicInfo,
            (&mut info as *mut FILE_BASIC_INFO).cast(),
            size_of::<FILE_BASIC_INFO>() as u32,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok(info)
}
pub(super) fn same_entry(path: &Path, expected: &File) -> bool {
    let Ok(info) = expected.metadata() else {
        return false;
    };
    let Ok(current) = open(
        path,
        FILE_READ_ATTRIBUTES | READ_CONTROL,
        FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
        info.is_dir(),
    ) else {
        return false;
    };
    matches!((identity(&current), identity(expected)), (Ok(actual), Ok(expected)) if actual == expected)
}

//! Windows Java 目录别名：只接受同一真实目录的已有 ASCII 路径，不改变卷设置。
use std::{
    ffi::OsString,
    fs::File,
    io,
    mem::size_of,
    os::windows::{
        ffi::{OsStrExt, OsStringExt},
        io::{AsRawHandle, FromRawHandle, OwnedHandle},
    },
    path::{Component, Path, PathBuf, Prefix},
};
use windows_sys::Win32::{Foundation::INVALID_HANDLE_VALUE, Storage::FileSystem::*};

fn invalid() -> io::Error {
    io::ErrorKind::InvalidInput.into()
}

fn checked(path: &Path) -> io::Result<&str> {
    if !path.is_absolute()
        || matches!(path.components().next(), Some(Component::Prefix(prefix)) if matches!(
            prefix.kind(), Prefix::Verbatim(_) | Prefix::VerbatimUNC(_, _) | Prefix::VerbatimDisk(_) | Prefix::DeviceNS(_)
        ))
    {
        return Err(invalid());
    }
    path.to_str()
        .filter(|text| text.is_ascii() && !text.chars().any(|ch| ch == '"' || ch.is_control()))
        .ok_or_else(invalid)
}

/// 选择系统已有的 ASCII 目录名，并通过句柄身份确认仍指向原目录。
/// 短名不存在、仍含非 ASCII、含不安全前缀或不能证明身份时明确失败。
pub fn ascii_directory(root: &Path) -> io::Result<PathBuf> {
    let pinned = open_directory(root)?;
    let simplified = crate::utils::strip_unc_prefix(root);
    if checked(&simplified).is_ok() {
        return validate_alias(root, &pinned, simplified);
    }
    // 系统返回的扩展路径仅交给 Windows API，不直接交给 Java。
    let native = std::fs::canonicalize(root)?;
    let input: Vec<_> = native.as_os_str().encode_wide().chain(Some(0)).collect();
    let length = unsafe { GetShortPathNameW(input.as_ptr(), std::ptr::null_mut(), 0) };
    if !(2..=32768).contains(&length) {
        return Err(invalid());
    }
    let mut output = vec![0u16; length as usize];
    let count = unsafe { GetShortPathNameW(input.as_ptr(), output.as_mut_ptr(), length) };
    if count == 0 || count >= length {
        return Err(invalid());
    }
    let alias =
        crate::utils::strip_unc_prefix(Path::new(&OsString::from_wide(&output[..count as usize])));
    validate_alias(root, &pinned, alias)
}

fn validate_alias(root: &Path, pinned: &File, alias: PathBuf) -> io::Result<PathBuf> {
    checked(&alias)?;
    if !matches_directory(root, pinned)? || !matches_directory(&alias, pinned)? {
        return Err(invalid());
    }
    Ok(alias)
}

/// 只读核对真实目录身份；不替代调用方的目录权限或任务生命周期检查。
pub fn same_directory(first: &Path, second: &Path) -> io::Result<bool> {
    let pinned = open_directory(first)?;
    Ok(matches_directory(first, &pinned)? && matches_directory(second, &pinned)?)
}

pub(crate) fn open_directory(path: &Path) -> io::Result<File> {
    if !path.is_absolute() {
        return Err(invalid());
    }
    // 保留最后一项，不把重解析点经 canonicalize 偷换成可接受的目录。
    let native = if let Some(name) = path.file_name() {
        std::fs::canonicalize(path.parent().ok_or_else(invalid)?)?.join(name)
    } else {
        std::fs::canonicalize(path)?
    };
    let input: Vec<_> = native.as_os_str().encode_wide().collect();
    if input.is_empty() || input.contains(&0) {
        return Err(invalid());
    }
    let input: Vec<_> = input.into_iter().chain(Some(0)).collect();
    // 兼容外层工作根已有的 DELETE 句柄；这里只核对身份，不声称锁住整个路径。
    let raw = unsafe {
        CreateFileW(
            input.as_ptr(),
            FILE_READ_ATTRIBUTES | READ_CONTROL,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            std::ptr::null(),
            OPEN_EXISTING,
            FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS,
            std::ptr::null_mut(),
        )
    };
    if raw == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error());
    }
    let file = File::from(unsafe { OwnedHandle::from_raw_handle(raw) });
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
        return Err(io::Error::last_os_error());
    }
    if unsafe { GetFileType(file.as_raw_handle()) } != FILE_TYPE_DISK
        || tag.FileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0
        || tag.FileAttributes & FILE_ATTRIBUTE_DIRECTORY == 0
    {
        return Err(invalid());
    }
    Ok(file)
}

pub(crate) fn identity(file: &File) -> io::Result<(u64, [u8; 16])> {
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
    Ok((info.VolumeSerialNumber, info.FileId.Identifier))
}

pub(crate) fn matches_directory(path: &Path, pinned: &File) -> io::Result<bool> {
    Ok(identity(&open_directory(path)?)? == identity(pinned)?)
}

#[cfg(test)]
mod tests;

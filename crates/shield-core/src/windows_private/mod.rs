//! Windows 私有临时材料的创建与权限校验；不修复已有权限，不拥有任务或清理权。
mod security;
pub use security::validate;
use security::PrivateDescriptor;
use std::{
    fs::File,
    io,
    os::windows::{
        ffi::OsStrExt,
        io::{FromRawHandle, OwnedHandle},
    },
    path::Path,
};
use windows_sys::Win32::{Foundation::INVALID_HANDLE_VALUE, Storage::FileSystem::*};

fn native_path(path: &Path) -> io::Result<Vec<u16>> {
    if !path.is_absolute() {
        return Err(io::ErrorKind::InvalidInput.into());
    }
    let name = path.file_name().ok_or(io::ErrorKind::InvalidInput)?;
    let units: Vec<_> = name.encode_wide().collect();
    // 不允许把“新建文件”变成向既有文件增加 NTFS 数据流，也不接受歧义末项。
    if units.is_empty()
        || units
            .iter()
            .any(|unit| *unit < 32 || b"<>:\"/\\|?*".iter().any(|byte| *unit == *byte as u16))
        || units
            .last()
            .is_some_and(|unit| *unit == b'.' as u16 || *unit == b' ' as u16)
    {
        return Err(io::ErrorKind::InvalidInput.into());
    }
    let native =
        std::fs::canonicalize(path.parent().ok_or(io::ErrorKind::InvalidInput)?)?.join(name);
    let wide: Vec<_> = native.as_os_str().encode_wide().collect();
    if wide.is_empty() || wide.contains(&0) {
        return Err(io::ErrorKind::InvalidInput.into());
    }
    Ok(wide.into_iter().chain(Some(0)).collect())
}

/// 独占新建目录，创建时仅授予当前用户和系统访问；已存在时失败，不改其权限。
pub fn create_directory(path: &Path) -> io::Result<()> {
    let native = native_path(path)?;
    let descriptor = PrivateDescriptor::new(true)?;
    if unsafe { CreateDirectoryW(native.as_ptr(), &descriptor.attributes()) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

/// 独占新建私有文件；读写／删除句柄由调用方管理，保留既有无覆盖提交能力。
pub fn create_file(path: &Path) -> io::Result<File> {
    let native = native_path(path)?;
    let descriptor = PrivateDescriptor::new(false)?;
    let raw = unsafe {
        CreateFileW(
            native.as_ptr(),
            FILE_GENERIC_READ | FILE_GENERIC_WRITE | DELETE,
            FILE_SHARE_READ,
            &descriptor.attributes(),
            CREATE_NEW,
            FILE_ATTRIBUTE_NORMAL | FILE_FLAG_OPEN_REPARSE_POINT,
            std::ptr::null_mut(),
        )
    };
    if raw == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error());
    }
    let file = File::from(unsafe { OwnedHandle::from_raw_handle(raw) });
    validate(&file, false)?;
    Ok(file)
}

/// 以只读、不跟随末项重解析点的方式打开，拒绝并行写入和删除共享。
pub(crate) fn readonly(path: &Path) -> io::Result<File> {
    let native = native_path(path)?;
    let raw = unsafe {
        CreateFileW(
            native.as_ptr(),
            FILE_GENERIC_READ | READ_CONTROL,
            FILE_SHARE_READ,
            std::ptr::null(),
            OPEN_EXISTING,
            FILE_FLAG_OPEN_REPARSE_POINT,
            std::ptr::null_mut(),
        )
    };
    if raw == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error());
    }
    Ok(File::from(unsafe { OwnedHandle::from_raw_handle(raw) }))
}

#[cfg(test)]
mod tests;

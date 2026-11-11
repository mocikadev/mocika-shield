//! Windows 任务文件与目录生命周期；只在本次句柄上提交，不覆盖或退回复制发布。
#[path = "windows/identity.rs"]
mod identity;
use identity::{kind, open, single_link, wide};
use std::{
    ffi::OsStr,
    fs::{self, File},
    io,
    mem::{offset_of, size_of},
    os::windows::{ffi::OsStrExt, io::AsRawHandle},
    path::{Path, PathBuf},
};
use windows_sys::Wdk::Storage::FileSystem::{
    FileRenameInformation, NtSetInformationFile, FILE_RENAME_INFORMATION,
};
use windows_sys::Win32::{
    Foundation::{RtlNtStatusToDosError, STATUS_SUCCESS},
    Storage::FileSystem::*,
    System::IO::IO_STATUS_BLOCK,
};

pub(crate) fn validate_name(name: &OsStr) -> io::Result<()> {
    let value = wide(name)?;
    let value = &value[..value.len() - 1];
    if value.len() > 255
        || value
            .iter()
            .any(|v| *v < 32 || b"<>:\"/\\|?*".iter().any(|c| *v == *c as u16))
        || value
            .last()
            .is_some_and(|v| *v == b'.' as u16 || *v == b' ' as u16)
    {
        return Err(io::ErrorKind::InvalidInput.into());
    }
    let base = name
        .to_string_lossy()
        .split('.')
        .next()
        .unwrap_or("")
        .to_ascii_uppercase();
    if matches!(
        base.as_str(),
        "CON" | "PRN" | "AUX" | "NUL" | "CLOCK$" | "CONIN$" | "CONOUT$"
    ) || ["COM", "LPT"].iter().any(|prefix| {
        base.strip_prefix(prefix).is_some_and(|tail| {
            matches!(
                tail,
                "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³"
            )
        })
    }) {
        return Err(io::ErrorKind::InvalidInput.into());
    }
    Ok(())
}

pub(in crate::automation) fn open_directory(path: &Path) -> io::Result<File> {
    // 不共享删除，任务期间父目录和私有暂存不能被改名后换成另一个对象。
    open(
        path,
        FILE_GENERIC_READ | FILE_GENERIC_WRITE | READ_CONTROL,
        FILE_SHARE_READ | FILE_SHARE_WRITE,
        true,
    )
}
pub(in crate::automation) fn private_directory(path: &Path) -> io::Result<()> {
    shield_core::windows_private::create_directory(path)
}
pub(in crate::automation) fn validate_directory(path: &Path) -> io::Result<()> {
    // 纯校验不申请写入、不建立新目录锁，且兼容暂存根已经持有的 DELETE 句柄。
    let file = inspect_directory(path)?;
    shield_core::windows_private::validate(&file, true)
}
pub(crate) fn inspect_directory(path: &Path) -> io::Result<File> {
    open(
        path,
        FILE_READ_ATTRIBUTES | READ_CONTROL,
        FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
        true,
    )
}
pub(crate) fn regular(path: &Path) -> io::Result<File> {
    // 拒绝仍由其他写入句柄持有的材料，不在流式读取期间允许修改或替换。
    open(
        path,
        FILE_GENERIC_READ | READ_CONTROL,
        FILE_SHARE_READ,
        false,
    )
}
pub(in crate::automation) fn create_at(
    directory: &File,
    path: &Path,
    name: &OsStr,
) -> io::Result<File> {
    validate_name(name)?;
    if !same_entry(path, directory) {
        return Err(io::ErrorKind::InvalidData.into());
    }
    shield_core::windows_private::create_file(&path.join(name))
}
pub(in crate::automation) fn artifact(directory: &File, path: &Path) -> io::Result<File> {
    if !same_entry(path, directory) {
        return Err(io::ErrorKind::InvalidData.into());
    }
    let file = regular(&path.join("signed.apk"))?;
    single_link(&file)?;
    if file.metadata()?.len() == 0 {
        return Err(io::ErrorKind::InvalidData.into());
    }
    Ok(file)
}
pub(in crate::automation) fn validate_private_file(file: &File) -> io::Result<()> {
    kind(file, false)?;
    single_link(file)?;
    shield_core::windows_private::validate(file, false)
}
pub(crate) fn same_entry(path: &Path, file: &File) -> bool {
    identity::same_entry(path, file)
}

pub(in crate::automation) fn file_stamp(
    path: &Path,
) -> io::Result<crate::automation::task_files::FileStamp> {
    identity::stamp(path)
}

#[cfg(test)]
pub(in crate::automation) fn same_directory(first: &Path, second: &Path) -> io::Result<bool> {
    shield_core::java_directory::same_directory(first, second)
}
#[cfg(any(test, feature = "mcp-server"))]
pub(crate) fn file_identity(
    file: &File,
) -> io::Result<crate::automation::task_files::FileIdentity> {
    identity::identity(file)
}

#[cfg(feature = "mcp-server")]
pub(crate) fn stamp_file(file: &File) -> io::Result<super::FileStamp> {
    identity::stamp_file(file)
}

pub(in crate::automation) struct WorkDirectory {
    path: PathBuf,
    directory: Option<File>,
    preserved: bool,
}
impl WorkDirectory {
    pub(in crate::automation) fn new(path: &Path, parent: &File) -> io::Result<Self> {
        if !same_entry(path, parent) {
            return Err(io::ErrorKind::InvalidData.into());
        }
        for _ in 0..128 {
            let suffix = crate::automation::random_id::create()?;
            let candidate = path.join(format!(".mocika-task-{suffix}"));
            match private_directory(&candidate) {
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error),
                Ok(()) => {}
            }
            // 未取得固定身份时不按路径删除，不能把被替换的目录当成本次新建目录。
            let directory = open(
                &candidate,
                FILE_GENERIC_READ | FILE_GENERIC_WRITE | DELETE,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                true,
            )?;
            let mut work = Self {
                path: candidate,
                directory: Some(directory),
                preserved: true,
            };
            if !same_entry(path, parent) || !same_entry(&work.path, work.directory()) {
                return Err(io::ErrorKind::InvalidData.into());
            }
            shield_core::windows_private::validate(work.directory(), true)?;
            work.preserved = false;
            return Ok(work);
        }
        Err(io::ErrorKind::AlreadyExists.into())
    }
    pub(in crate::automation) fn path(&self) -> &Path {
        &self.path
    }
    pub(in crate::automation) fn directory(&self) -> &File {
        self.directory.as_ref().expect("活动暂存持有目录句柄")
    }
    pub(in crate::automation) fn keep(mut self) -> PathBuf {
        self.preserved = true;
        self.path.clone()
    }
    pub(in crate::automation) fn close(mut self) -> io::Result<()> {
        self.preserved = true;
        self.remove()
    }
    fn remove(&mut self) -> io::Result<()> {
        let file = self.directory.take().ok_or(io::ErrorKind::InvalidData)?;
        if !same_entry(&self.path, &file) {
            return Err(io::ErrorKind::InvalidData.into());
        }
        // 根目录一直禁止改名；标准库删除子项不跟随目录链接，不碰本次根之外的路径。
        for entry in fs::read_dir(&self.path)? {
            let entry = entry?;
            if entry.file_type()?.is_dir() {
                fs::remove_dir_all(entry.path())?;
            } else {
                fs::remove_file(entry.path())?;
            }
        }
        // 用本次 DELETE 句柄删除空根，而不是放开根锁后按路径递归删除。
        let info = FILE_DISPOSITION_INFO { DeleteFile: true };
        if unsafe {
            SetFileInformationByHandle(
                file.as_raw_handle(),
                FileDispositionInfo,
                (&info as *const FILE_DISPOSITION_INFO).cast(),
                size_of::<FILE_DISPOSITION_INFO>() as u32,
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        drop(file);
        match fs::symlink_metadata(&self.path) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error),
            Ok(_) => Err(io::ErrorKind::AlreadyExists.into()),
        }
    }
}
impl Drop for WorkDirectory {
    fn drop(&mut self) {
        if !self.preserved {
            let _ = self.remove();
        }
    }
}

pub(in crate::automation) fn commit_no_replace(
    _directory: &File,
    _source: &OsStr,
    sealed: &File,
    parent: &File,
    target: &OsStr,
) -> io::Result<()> {
    validate_name(target)?;
    let name: Vec<u16> = target.encode_wide().collect();
    let offset = offset_of!(FILE_RENAME_INFORMATION, FileName);
    let bytes = size_of::<FILE_RENAME_INFORMATION>() + (name.len() + 1) * size_of::<u16>();
    let mut storage = vec![0usize; bytes.div_ceil(size_of::<usize>())];
    // 布局、字段偏移和对齐来自 SDK；只接受单个文件名，不能注入目录或 NTFS 数据流。
    let info = storage.as_mut_ptr().cast::<FILE_RENAME_INFORMATION>();
    unsafe {
        (*info).Anonymous.ReplaceIfExists = false;
        (*info).RootDirectory = parent.as_raw_handle();
        (*info).FileNameLength = (name.len() * size_of::<u16>()) as u32;
        std::ptr::copy_nonoverlapping(
            name.as_ptr(),
            storage.as_mut_ptr().byte_add(offset).cast::<u16>(),
            name.len(),
        );
    }
    let mut completed = IO_STATUS_BLOCK::default();
    // 封存句柄由本模块同步创建；参数在系统调用完成前存活。直接使用 SDK 的用户态
    // 普通重命名类，不使用绕过访问校验类，避免 Win32 包装层的相对路径处理差异。
    let status = unsafe {
        NtSetInformationFile(
            sealed.as_raw_handle(),
            &mut completed,
            storage.as_ptr().cast(),
            bytes as u32,
            FileRenameInformation,
        )
    };
    if status != STATUS_SUCCESS {
        #[cfg(test)]
        eprintln!("本次原生句柄提交失败，状态码：0x{:08x}", status as u32);
        return Err(io::Error::from_raw_os_error(
            unsafe { RtlNtStatusToDosError(status) } as i32,
        ));
    }
    Ok(())
}
pub(in crate::automation) fn complete_materials(directory: &File) -> io::Result<()> {
    // 文件已分别 sync_all；Windows 不伪称支持 Unix 目录 fsync，材料只用于本次运行。
    shield_core::windows_private::validate(directory, true)
}
pub(in crate::automation) fn sync_directory(_directory: &File) -> bool {
    false
}

#[cfg(test)]
pub(in crate::automation) fn test_make_public(path: &Path, directory: bool) {
    let file = open(
        path,
        WRITE_DAC | READ_CONTROL,
        FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
        directory,
    )
    .unwrap();
    permission_fixture::make_public(&file);
}

#[cfg(test)]
#[allow(dead_code)]
mod permission_fixture {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/fixtures/windows_permissions.rs"
    ));
}

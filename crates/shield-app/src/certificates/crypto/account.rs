//! 缺少用户名环境时查询当前有效账户；不修改环境、不启动外部命令。
use std::ffi::OsString;

#[cfg(unix)]
pub(super) fn current_name() -> Option<OsString> {
    use std::{ffi::CStr, mem::MaybeUninit, os::unix::ffi::OsStringExt, ptr};
    let mut size = 1024;
    while size <= 65536 {
        let mut buffer = vec![0u8; size];
        let mut record = MaybeUninit::<libc::passwd>::uninit();
        let mut result = ptr::null_mut();
        // getpwuid_r 使用调用者拥有的缓冲区，无全局 passwd 存储；指针仅在本次调用内有效。
        let status = unsafe {
            libc::getpwuid_r(
                libc::geteuid(),
                record.as_mut_ptr(),
                buffer.as_mut_ptr().cast(),
                buffer.len(),
                &mut result,
            )
        };
        if status == libc::ERANGE {
            size *= 2;
            continue;
        }
        if status != 0 || result.is_null() {
            return None;
        }
        // 成功且非空时系统已初始化 record，pw_name 为缓冲区内以 NUL 结尾的账户名。
        let record = unsafe { record.assume_init() };
        if record.pw_name.is_null() {
            return None;
        }
        let bytes = unsafe { CStr::from_ptr(record.pw_name) }.to_bytes();
        return (!bytes.is_empty()).then(|| OsString::from_vec(bytes.to_vec()));
    }
    None
}

#[cfg(not(unix))]
pub(super) fn current_name() -> Option<OsString> {
    None
}

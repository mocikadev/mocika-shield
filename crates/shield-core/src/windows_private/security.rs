//! 仅本次私有材料的 Windows 访问控制，不改变应用资料库或系统权限。
use std::{
    fs::File,
    io,
    mem::{offset_of, size_of},
    os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle},
};
use windows_sys::Win32::{
    Foundation::LocalFree,
    Security::{
        Authorization::{
            ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW,
            GetSecurityInfo, SE_FILE_OBJECT,
        },
        *,
    },
    System::{
        SystemServices::ACCESS_ALLOWED_ACE_TYPE,
        Threading::{GetCurrentProcess, OpenProcessToken},
    },
};

pub(super) fn denied() -> io::Error {
    io::ErrorKind::PermissionDenied.into()
}

pub(super) struct Local(pub(super) *mut std::ffi::c_void);
impl Drop for Local {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe {
                LocalFree(self.0);
            }
        }
    }
}

struct User(Vec<u32>);
impl User {
    fn sid(&self) -> PSID {
        self.0.as_ptr().cast_mut().cast()
    }
    fn current() -> io::Result<Self> {
        let mut token = std::ptr::null_mut();
        if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
            return Err(io::Error::last_os_error());
        }
        let token = unsafe { OwnedHandle::from_raw_handle(token) };
        // TOKEN_USER 与 SID 很小，拒绝超出固定预算的数据，不按报告长度无界分配。
        let mut buffer = [0usize; 64];
        let mut written = 0;
        if unsafe {
            GetTokenInformation(
                token.as_raw_handle(),
                TokenUser,
                buffer.as_mut_ptr().cast(),
                size_of::<[usize; 64]>() as u32,
                &mut written,
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        if !(size_of::<TOKEN_USER>()..=size_of::<[usize; 64]>()).contains(&(written as usize)) {
            return Err(denied());
        }
        let info = unsafe { &*buffer.as_ptr().cast::<TOKEN_USER>() };
        let begin = buffer.as_ptr() as usize;
        let sid = info.User.Sid as usize;
        if sid < begin
            || sid
                .checked_add(8)
                .is_none_or(|end| end > begin + written as usize)
        {
            return Err(denied());
        }
        let length = unsafe { GetLengthSid(info.User.Sid) } as usize;
        if !(8..=68).contains(&length)
            || sid + length > begin + written as usize
            || unsafe { IsValidSid(info.User.Sid) } == 0
        {
            return Err(denied());
        }
        let mut copy = vec![0u32; length.div_ceil(size_of::<u32>())];
        unsafe {
            std::ptr::copy_nonoverlapping(
                info.User.Sid.cast::<u8>(),
                copy.as_mut_ptr().cast(),
                length,
            );
        }
        Ok(Self(copy))
    }
    fn text(&self) -> io::Result<String> {
        let mut raw = std::ptr::null_mut();
        if unsafe { ConvertSidToStringSidW(self.sid(), &mut raw) } == 0 {
            return Err(io::Error::last_os_error());
        }
        let _memory = Local(raw.cast());
        // SID 字符串最多固定预算；只用于本机安全描述符，不进入日志或协议。
        let mut units = Vec::new();
        for index in 0..192 {
            let value = unsafe { *raw.add(index) };
            if value == 0 {
                return String::from_utf16(&units).map_err(|_| denied());
            }
            units.push(value);
        }
        Err(denied())
    }
}

pub(super) struct PrivateDescriptor(Local);
impl PrivateDescriptor {
    pub(super) fn new(directory: bool) -> io::Result<Self> {
        let user = User::current()?.text()?;
        let inherit = if directory { "OICI" } else { "" };
        let text = format!("O:{user}D:P(A;{inherit};FA;;;{user})(A;{inherit};FA;;;SY)");
        let wide: Vec<u16> = text.encode_utf16().chain(Some(0)).collect();
        let mut raw = std::ptr::null_mut();
        if unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                wide.as_ptr(),
                1,
                &mut raw,
                std::ptr::null_mut(),
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(Self(Local(raw)))
    }
    pub(super) fn attributes(&self) -> SECURITY_ATTRIBUTES {
        SECURITY_ATTRIBUTES {
            nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: self.0 .0,
            bInheritHandle: 0,
        }
    }
}

/// 核对已有句柄的所有者与私有访问控制，不修改权限；目录还必须禁止继承宽泛 ACL。
pub fn validate(file: &File, directory: bool) -> io::Result<()> {
    let user = User::current()?;
    let mut owner = std::ptr::null_mut();
    let mut acl = std::ptr::null_mut();
    let mut raw = std::ptr::null_mut();
    // SDK 分配的描述符同时拥有 owner 和 ACL 的存储，校验期间始终保留。
    let error = unsafe {
        GetSecurityInfo(
            file.as_raw_handle(),
            SE_FILE_OBJECT,
            OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
            &mut owner,
            std::ptr::null_mut(),
            &mut acl,
            std::ptr::null_mut(),
            &mut raw,
        )
    };
    if error != 0 {
        return Err(io::Error::from_raw_os_error(error as i32));
    }
    let descriptor = Local(raw);
    if descriptor.0.is_null()
        || owner.is_null()
        || acl.is_null()
        || unsafe { IsValidSecurityDescriptor(descriptor.0) } == 0
        || unsafe { EqualSid(owner, user.sid()) } == 0
    {
        return Err(denied());
    }
    let mut control = 0;
    let mut revision = 0;
    if unsafe { GetSecurityDescriptorControl(descriptor.0, &mut control, &mut revision) } == 0 {
        return Err(denied());
    }
    if directory && control & SE_DACL_PROTECTED == 0 {
        return Err(denied());
    }
    let count = unsafe { (*acl).AceCount };
    if count == 0 || count > 8 {
        return Err(denied());
    }
    let mut has_user = false;
    for index in 0..count {
        let mut raw_ace = std::ptr::null_mut();
        if unsafe { GetAce(acl, index as u32, &mut raw_ace) } == 0 || raw_ace.is_null() {
            return Err(denied());
        }
        let header = unsafe { &*raw_ace.cast::<ACE_HEADER>() };
        let offset = offset_of!(ACCESS_ALLOWED_ACE, SidStart);
        if header.AceType != ACCESS_ALLOWED_ACE_TYPE as u8 || (header.AceSize as usize) < offset + 8
        {
            return Err(denied());
        }
        let sid = unsafe { raw_ace.byte_add(offset) };
        let length = unsafe { GetLengthSid(sid) } as usize;
        if !(8..=68).contains(&length)
            || offset + length > header.AceSize as usize
            || unsafe { IsValidSid(sid) } == 0
        {
            return Err(denied());
        }
        let owned = unsafe { EqualSid(sid, user.sid()) } != 0;
        if !owned && unsafe { IsWellKnownSid(sid, WinLocalSystemSid) } == 0 {
            return Err(denied());
        }
        has_user |= owned;
    }
    if !has_user {
        return Err(denied());
    }
    Ok(())
}

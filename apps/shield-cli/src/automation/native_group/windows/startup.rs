//! 本次进程的可继承标准流和创建属性；Job 句柄不进入继承白名单。
use crate::automation::windows_job::Job;
use std::{
    fs::{File, OpenOptions},
    io,
    mem::size_of,
    os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle},
};
use windows_sys::Win32::{
    Foundation::{
        DuplicateHandle, SetHandleInformation, DUPLICATE_SAME_ACCESS, HANDLE_FLAG_INHERIT,
    },
    Security::SECURITY_ATTRIBUTES,
    System::{Pipes::CreatePipe, Threading::*},
};

pub(super) struct Startup {
    pub input: File,
    pub output: File,
    child_input: File,
    child_output: File,
    error: OwnedHandle,
}
impl Startup {
    pub fn new() -> io::Result<Self> {
        let (child_input, input) = pipe(false)?;
        let (output, child_output) = pipe(true)?;
        let null = OpenOptions::new().write(true).open("NUL")?;
        let mut error = std::ptr::null_mut();
        let current = unsafe { GetCurrentProcess() };
        if unsafe {
            DuplicateHandle(
                current,
                null.as_raw_handle(),
                current,
                &mut error,
                0,
                1,
                DUPLICATE_SAME_ACCESS,
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(Self {
            input,
            output,
            child_input,
            child_output,
            error: unsafe { OwnedHandle::from_raw_handle(error) },
        })
    }

    pub fn create(
        &self,
        job: &Job,
        parameters: &mut super::parameters::Parameters,
    ) -> io::Result<PROCESS_INFORMATION> {
        let mut attributes = Attributes::new()?;
        let handles = [
            self.child_input.as_raw_handle(),
            self.child_output.as_raw_handle(),
            self.error.as_raw_handle(),
        ];
        let jobs = [job.as_raw_handle()];
        attributes.add(PROC_THREAD_ATTRIBUTE_HANDLE_LIST, &handles)?;
        attributes.add(PROC_THREAD_ATTRIBUTE_JOB_LIST, &jobs)?;
        let mut info = STARTUPINFOEXW::default();
        info.StartupInfo.cb = size_of::<STARTUPINFOEXW>() as u32;
        info.StartupInfo.dwFlags = STARTF_USESTDHANDLES;
        info.StartupInfo.hStdInput = handles[0];
        info.StartupInfo.hStdOutput = handles[1];
        info.StartupInfo.hStdError = handles[2];
        info.lpAttributeList = attributes.pointer();
        let mut process = PROCESS_INFORMATION::default();
        // 安全：SDK 结构与属性所借用的数组全程存活；完整应用路径，无 shell，无恢复运行窗口。
        let ok = unsafe {
            CreateProcessW(
                parameters.program.as_ptr(),
                parameters.arguments.as_mut_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                1,
                CREATE_NO_WINDOW | CREATE_UNICODE_ENVIRONMENT | EXTENDED_STARTUPINFO_PRESENT,
                parameters.environment.as_ptr().cast(),
                parameters.directory.as_ptr(),
                &info.StartupInfo,
                &mut process,
            )
        };
        if ok == 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(process)
        }
    }
}

fn pipe(parent_reads: bool) -> io::Result<(File, File)> {
    let security = SECURITY_ATTRIBUTES {
        nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: std::ptr::null_mut(),
        bInheritHandle: 1,
    };
    let (mut read, mut write) = (std::ptr::null_mut(), std::ptr::null_mut());
    if unsafe { CreatePipe(&mut read, &mut write, &security, 0) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let read = File::from(unsafe { OwnedHandle::from_raw_handle(read) });
    let write = File::from(unsafe { OwnedHandle::from_raw_handle(write) });
    let parent = if parent_reads { &read } else { &write };
    if unsafe { SetHandleInformation(parent.as_raw_handle(), HANDLE_FLAG_INHERIT, 0) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok((read, write))
}

struct Attributes {
    storage: Vec<usize>,
    initialized: bool,
}
impl Attributes {
    fn pointer(&mut self) -> LPPROC_THREAD_ATTRIBUTE_LIST {
        self.storage.as_mut_ptr().cast()
    }
    fn new() -> io::Result<Self> {
        let mut bytes = 0;
        unsafe {
            InitializeProcThreadAttributeList(std::ptr::null_mut(), 2, 0, &mut bytes);
        }
        if bytes == 0 || bytes > 64 * 1024 {
            return Err(io::ErrorKind::InvalidData.into());
        }
        let mut attributes = Self {
            storage: vec![0; bytes.div_ceil(size_of::<usize>())],
            initialized: false,
        };
        if unsafe { InitializeProcThreadAttributeList(attributes.pointer(), 2, 0, &mut bytes) } == 0
        {
            // 初始化失败不能调用 Delete；未初始化缓冲区只交给 Vec 回收。
            return Err(io::Error::last_os_error());
        }
        attributes.initialized = true;
        Ok(attributes)
    }
    fn add<T>(&mut self, key: u32, value: &T) -> io::Result<()> {
        if unsafe {
            UpdateProcThreadAttribute(
                self.pointer(),
                0,
                key as usize,
                (value as *const T).cast(),
                size_of::<T>(),
                std::ptr::null_mut(),
                std::ptr::null(),
            )
        } == 0
        {
            Err(io::Error::last_os_error())
        } else {
            Ok(())
        }
    }
}
impl Drop for Attributes {
    fn drop(&mut self) {
        if self.initialized {
            unsafe {
                DeleteProcThreadAttributeList(self.pointer());
            }
        }
    }
}

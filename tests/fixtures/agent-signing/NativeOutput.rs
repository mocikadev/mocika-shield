//! 仅供原生故障代理隔离自身输出继承；不进入正式工具。
use std::{fs, path::Path};
pub(super) fn isolate(root: &Path) {
    // 签名与常量按锁定的 windows-sys 元数据；仅修改本测试代理自己的标准流句柄。
    #[link(name = "kernel32")]
    extern "system" {
        fn GetStdHandle(kind: u32) -> *mut std::ffi::c_void;
        fn GetHandleInformation(handle: *mut std::ffi::c_void, flags: *mut u32) -> i32;
        fn SetHandleInformation(handle: *mut std::ffi::c_void, mask: u32, flags: u32) -> i32;
    }
    let mut inherited_output = false;
    for kind in [-11i32, -12i32] {
        let handle = unsafe { GetStdHandle(kind as u32) };
        assert!(!handle.is_null() && handle as isize != -1);
        let mut flags = 0;
        assert_ne!(unsafe { GetHandleInformation(handle, &mut flags) }, 0);
        if kind == -11 {
            inherited_output = flags & 1 != 0;
        }
        assert_ne!(unsafe { SetHandleInformation(handle, 1, 0) }, 0);
        assert_ne!(unsafe { GetHandleInformation(handle, &mut flags) }, 0);
        assert_eq!(flags & 1, 0);
    }
    fs::write(
        root.join("proxy-output-flags"),
        format!("{{\"before\":{inherited_output},\"after\":false}}"),
    )
    .unwrap();
}

//! 私有输入的身份、权限与链接负向测试；不启动工具、不操作真实资料。
use super::*;

#[cfg(windows)]
#[allow(dead_code)]
mod permissions {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/fixtures/windows_permissions.rs"
    ));
}

#[test]
fn 同内容同长度替换也不能通过请求身份核对() {
    let root = private_root();
    let path = root.path().join("request.bin");
    let bytes = request::encode(&["hello"]).unwrap();
    let mut writer = private_io::create_file(&path).unwrap();
    writer.write_all(&bytes).unwrap();
    let identity = private_io::identity(&writer).unwrap();
    drop(writer);
    fs::rename(&path, root.path().join("original.bin")).unwrap();
    let mut replacement = private_io::create_file(&path).unwrap();
    replacement.write_all(&bytes).unwrap();
    drop(replacement);
    assert!(private_io::read_back(&path, identity, bytes.len()).is_err());
    assert_eq!(fs::read(root.path().join("original.bin")).unwrap(), bytes);
    assert_eq!(fs::read(&path).unwrap(), bytes);
}

#[test]
fn 硬链接和已有目录不能充当私有请求() {
    let root = private_root();
    let path = root.path().join("request.bin");
    let mut writer = private_io::create_file(&path).unwrap();
    writer.write_all(b"private").unwrap();
    drop(writer);
    let link = root.path().join("linked.bin");
    fs::hard_link(&path, &link).unwrap();
    assert!(private_io::readonly(&path).is_err());
    assert!(private_io::readonly(&link).is_err());
    assert!(request::input(root.path(), b"changed").is_err());
    assert_eq!(fs::read(&path).unwrap(), b"private");
    let other = root.path().join("directory");
    create_workspace(&other).unwrap();
    create_workspace(&other.join("request.bin")).unwrap();
    assert!(request::input(&other, b"changed").is_err());
}

#[cfg(unix)]
#[test]
fn 符号链接和fifo不能充当输入且权限放宽后不启动() {
    use std::{
        ffi::CString,
        os::unix::fs::{symlink, PermissionsExt},
    };
    let root = private_root();
    let path = root.path().join("private.bin");
    drop(private_io::create_file(&path).unwrap());
    let link = root.path().join("link.bin");
    symlink(&path, &link).unwrap();
    assert!(private_io::readonly(&link).is_err());
    let fifo = root.path().join("fifo");
    let native = CString::new(fifo.as_os_str().as_encoded_bytes()).unwrap();
    assert_eq!(unsafe { libc::mkfifo(native.as_ptr(), 0o600) }, 0);
    assert!(private_io::readonly(&fifo).is_err());
    fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
    assert!(private_io::readonly(&path).is_err());
    let jar = root.path().join("adapter.jar");
    fs::write(&jar, fixture(env!("CARGO_PKG_VERSION"), "")).unwrap();
    let context = JavaToolContext::new(&jar, root.path()).unwrap();
    fs::set_permissions(root.path(), fs::Permissions::from_mode(0o755)).unwrap();
    assert!(context.prepare(&std::env::current_exe().unwrap()).is_err());
    assert!(request::input(root.path(), b"changed").is_err());
    assert_eq!(
        fs::metadata(root.path()).unwrap().permissions().mode() & 0o777,
        0o755
    );
}

#[cfg(windows)]
#[test]
fn 借用根权限放宽后拒绝准备且不修复原目录() {
    let root = private_root();
    let jar = root.path().join("adapter.jar");
    fs::write(&jar, fixture(env!("CARGO_PKG_VERSION"), "")).unwrap();
    let context = JavaToolContext::new(&jar, root.path()).unwrap();
    let control = permissions::control(root.path(), true);
    permissions::make_public(&control);
    assert!(context.prepare(&std::env::current_exe().unwrap()).is_err());
    assert!(JavaToolContext::new(&jar, root.path()).is_err());
    assert!(request::input(root.path(), b"changed").is_err());
    assert!(crate::windows_private::validate(&control, true).is_err());
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 1);
}

#[cfg(windows)]
#[test]
fn 私有请求只读共享阻止修改且权限放宽后仍拒绝() {
    use std::os::windows::fs::OpenOptionsExt;
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
    };
    let root = private_root();
    let bytes = request::encode(&["hello"]).unwrap();
    let mut input = request::input(root.path(), &bytes).unwrap();
    let path = root.path().join("request.bin");
    assert!(fs::OpenOptions::new()
        .write(true)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
        .open(&path)
        .is_err());
    assert!(fs::rename(&path, root.path().join("moved.bin")).is_err());
    assert!(input.write_all(b"changed").is_err());
    let control = permissions::control(&path, false);
    permissions::make_public(&control);
    assert!(private_io::validate_file(&input).is_err());
    assert!(private_io::readonly(&path).is_err());
    assert_eq!(fs::read(&path).unwrap(), bytes);
}

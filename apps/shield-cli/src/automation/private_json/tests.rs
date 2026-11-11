use super::*;
use serde_json::{json, Value};
use std::fs;

#[test]
fn 中文内容原字节摘要及读取长度边界一致且不覆盖已有文件() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("中文 任务.json");
    let value = json!({"version":1,"value":"中文 空格"});
    let bytes = serde_json::to_vec(&value).unwrap();
    let digest = write(&path, &value).ok().unwrap();
    assert_eq!(digest.as_slice(), Sha256::digest(&bytes).as_slice());
    let (actual, raw) = read::<Value>(&path, bytes.len() as u64).ok().unwrap();
    assert_eq!(actual, value);
    assert_eq!(raw, bytes);
    assert!(read::<Value>(&path, bytes.len() as u64 - 1).is_err());
    assert!(write(&path, &json!({"changed":true})).is_err());
    assert_eq!(fs::read(&path).unwrap(), bytes);
    assert!(read::<Value>(&path, u64::MAX).is_err());
    let oversized = root.path().join("oversized.json");
    assert!(write(&oversized, &"x".repeat(MAX_BYTES as usize)).is_err());
    assert!(!oversized.exists());
}

#[test]
fn 活动任务目录锁下读写私有json且可正常收尾() {
    let root = tempfile::tempdir().unwrap();
    let parent = task_files::open_directory(root.path()).unwrap();
    let work = task_files::WorkDirectory::new(root.path(), &parent).unwrap();
    let path = work.path().join("bootstrap.json");
    assert!(write(&path, &json!({"fixed":true})).is_ok());
    assert!(read::<Value>(&path, MAX_BYTES).is_ok());
    work.close().unwrap();
}

#[test]
fn 损坏内容链接非普通文件和权限放宽均拒绝且不回传原文() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("private.json");
    assert!(write(&path, &json!({"fixed":true})).is_ok());
    let linked = root.path().join("hard.json");
    fs::hard_link(&path, &linked).unwrap();
    assert!(read::<Value>(&path, MAX_BYTES).is_err());
    fs::remove_file(linked).unwrap();
    let linked = root.path().join("soft.json");
    #[cfg(unix)]
    std::os::unix::fs::symlink(&path, &linked).unwrap();
    #[cfg(windows)]
    std::os::windows::fs::symlink_file(&path, &linked).unwrap();
    assert!(read::<Value>(&linked, MAX_BYTES).is_err());
    assert!(write(&linked, &json!(false)).is_err());
    assert!(read::<Value>(root.path(), MAX_BYTES).is_err());
    fs::write(&path, b"not-json:fixture-only").unwrap();
    let error = read::<Value>(&path, MAX_BYTES).err().unwrap();
    assert_eq!(error.code, "PRIVATE_FILE_INVALID");
    assert!(!error.message.contains("fixture-only"));
    assert!(write(&root.path().join("public.json"), &json!(true)).is_ok());
    let public = root.path().join("public.json");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&public, fs::Permissions::from_mode(0o644)).unwrap();
    }
    #[cfg(windows)]
    task_files::test_make_public(&public, false);
    assert!(read::<Value>(&public, MAX_BYTES).is_err());
}

#[cfg(unix)]
#[test]
fn fifo不会阻塞有界私有读取() {
    use std::{ffi::CString, os::unix::ffi::OsStrExt};
    let root = tempfile::tempdir().unwrap();
    let fifo = root.path().join("fifo");
    let name = CString::new(fifo.as_os_str().as_bytes()).unwrap();
    assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
    let started = std::time::Instant::now();
    assert!(read::<Value>(&fifo, MAX_BYTES).is_err());
    assert!(started.elapsed() < std::time::Duration::from_secs(1));
}

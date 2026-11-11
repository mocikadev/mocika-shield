//! Windows 真实文件边界，不打开真实资料库、不运行 APK 或更改系统权限配置。
use super::*;
use std::os::windows::{fs::symlink_dir, fs::OpenOptionsExt};
use windows_sys::Win32::Storage::FileSystem::{FILE_SHARE_READ, FILE_SHARE_WRITE};

const CONTENT: &[u8] = b"isolated test bytes, not an apk";
fn prepared(root: &Path) -> Publication {
    let publication = Publication::new(&root.join("中文结果 带空格.apk")).unwrap();
    fs::write(publication.work_dir().unwrap().join("signed.apk"), CONTENT).unwrap();
    publication
}
fn digest() -> [u8; 32] {
    Sha256::digest(CONTENT).into()
}

#[test]
fn 输出父目录和暂存目录在任务期间不能被改名替换() {
    let root = tempfile::tempdir().unwrap();
    let parent = root.path().join("输出");
    fs::create_dir(&parent).unwrap();
    let mut publication = prepared(&parent);
    let work = publication.work_dir().unwrap().to_owned();
    assert!(fs::rename(&parent, root.path().join("被替换的父目录")).is_err());
    assert!(fs::rename(&work, parent.join("被替换的暂存")).is_err());
    let receipt = publication
        .publish_after_stop(digest(), |_| Ok(()))
        .unwrap();
    assert!(receipt.location_confirmed && receipt.cleanup_complete);
    assert_eq!(
        fs::read(parent.join("中文结果 带空格.apk")).unwrap(),
        CONTENT
    );
    assert!(!work.exists());
}

#[test]
fn 输出目录链接改向后拒绝且不写入新目录() {
    let root = tempfile::tempdir().unwrap();
    let first = root.path().join("原目录");
    let second = root.path().join("其他目录");
    fs::create_dir(&first).unwrap();
    fs::create_dir(&second).unwrap();
    let link = root.path().join("目录链接");
    symlink_dir(&first, &link).unwrap();
    let mut publication = prepared(&link);
    fs::remove_dir(&link).unwrap();
    symlink_dir(&second, &link).unwrap();
    assert!(matches!(
        publication.publish_after_stop(digest(), |_| Ok(())),
        Err(PublicationError::OutputChanged)
    ));
    assert!(publication.cleanup());
    assert_eq!(fs::read_dir(first).unwrap().count(), 0);
    assert_eq!(fs::read_dir(second).unwrap().count(), 0);
}

#[test]
fn 候选文件的写入句柄未释放时不封存或发布() {
    let root = tempfile::tempdir().unwrap();
    let mut publication = prepared(root.path());
    let writing = fs::OpenOptions::new()
        .write(true)
        .open(publication.work_dir().unwrap().join("signed.apk"))
        .unwrap();
    assert!(matches!(
        publication.publish_after_stop(digest(), |_| Ok(())),
        Err(PublicationError::InvalidArtifact)
    ));
    assert!(!publication.target.exists());
    drop(writing);
    assert!(publication.cleanup());
}

#[test]
fn 最后检查才出现的输出仍不能被覆盖() {
    let root = tempfile::tempdir().unwrap();
    let mut publication = prepared(root.path());
    let target = publication.target.clone();
    let result = publication.publish_after_stop(digest(), |phase| {
        if phase == PublicationCheckpoint::BeforeCommit {
            fs::write(&target, b"keep original").unwrap();
        }
        Ok(())
    });
    assert!(matches!(result, Err(PublicationError::OutputExists)));
    assert_eq!(fs::read(&target).unwrap(), b"keep original");
    assert!(publication.cleanup());
}

#[test]
fn 提交后清理失败保留已交付事实且不重试删除() {
    let root = tempfile::tempdir().unwrap();
    let mut publication = prepared(root.path());
    let work = publication.work_dir().unwrap().to_owned();
    let marker = work.join("仅本次锁定材料.txt");
    fs::write(&marker, b"keep").unwrap();
    let held = fs::OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
        .open(marker)
        .unwrap();
    let receipt = publication
        .publish_after_stop(digest(), |_| Ok(()))
        .unwrap();
    assert!(receipt.location_confirmed);
    assert!(!receipt.cleanup_complete);
    assert!(!publication.cleanup());
    assert_eq!(fs::read(&publication.target).unwrap(), CONTENT);
    assert!(work.exists());
    drop(held);
}

#[test]
fn 设备名数据流及路径注入不能作为最终文件名() {
    let root = tempfile::tempdir().unwrap();
    for name in [
        "CON.apk",
        "LPT1.apk",
        "COM¹.apk",
        "NUL.apk",
        "result.apk:extra",
    ] {
        assert!(matches!(
            Publication::new(&root.path().join(name)),
            Err(PublicationError::OutputInvalid)
        ));
    }
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 0);
}

#[test]
fn 长路径保留中文且不向最后一项的链接跟随() {
    let root = tempfile::tempdir().unwrap();
    let mut parent = root.path().to_owned();
    for _ in 0..5 {
        parent = parent.join("仅本次长路径验证_abcdefghijklmnopqrstuvwxyz0123456789");
    }
    fs::create_dir_all(&parent).unwrap();
    assert!(parent.as_os_str().len() > 260);
    let mut publication = prepared(&parent);
    let receipt = publication
        .publish_after_stop(digest(), |_| Ok(()))
        .unwrap();
    assert!(receipt.location_confirmed && receipt.cleanup_complete);
    assert_eq!(fs::read(&publication.target).unwrap(), CONTENT);
}

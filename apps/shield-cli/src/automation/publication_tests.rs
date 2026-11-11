//! 只验证本机文件边界；构造字节不是实际 APK，不证明工作进程停止或签名合格。
use super::*;
#[cfg(unix)]
use std::os::unix::fs::{symlink, PermissionsExt};
#[cfg(windows)]
use std::os::windows::fs::symlink_file as symlink;
use std::sync::{Arc, Barrier};

const CONTENT: &[u8] = "完整产物的测试字节，不是实际 APK".as_bytes();

#[test]
fn 无法确认停止时显式保留本次目录且析构不清理() {
    let root = tempfile::tempdir().unwrap();
    let mut publication = prepared(root.path());
    let work = publication.work_dir().unwrap().to_owned();
    publication.preserve();
    assert!(!publication.cleanup());
    drop(publication);
    assert_eq!(fs::read(work.join("signed.apk")).unwrap(), CONTENT);
}

fn digest(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
}

fn prepared(root: &Path) -> Publication {
    let publication = Publication::new(&root.join("最终中文输出 空格.apk")).unwrap();
    fs::write(publication.work_dir().unwrap().join("signed.apk"), CONTENT).unwrap();
    publication
}

#[test]
fn 同目录私有暂存且提交前没有最终文件() {
    let root = tempfile::tempdir().unwrap();
    let publication = prepared(root.path());
    let work = publication.work_dir().unwrap().to_owned();
    assert_eq!(
        work.parent(),
        Some(task_files::resolved(root.path()).unwrap().as_path())
    );
    #[cfg(unix)]
    assert_eq!(
        fs::metadata(&work).unwrap().permissions().mode() & 0o777,
        0o700
    );
    task_files::validate_directory(&work).unwrap();
    assert!(!publication.target.exists());
    drop(publication);
    assert!(!work.exists());
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 0);
}

#[test]
fn 完整提交固化字节并返回单独的收尾状态() {
    let root = tempfile::tempdir().unwrap();
    let mut publication = prepared(root.path());
    let target = publication.target.clone();
    let source = publication.work_dir().unwrap().join("signed.apk");
    let source_identity = task_files::file_identity(&File::open(&source).unwrap()).unwrap();
    let receipt = publication
        .publish_after_stop(digest(CONTENT), |_| Ok(()))
        .unwrap();
    assert_eq!(fs::read(&target).unwrap(), CONTENT);
    assert_eq!(receipt.bytes, CONTENT.len() as u64);
    assert_eq!(receipt.sha256, digest(CONTENT));
    assert!(receipt.location_confirmed);
    #[cfg(unix)]
    assert!(receipt.directory_synced);
    #[cfg(windows)]
    assert!(!receipt.directory_synced);
    assert!(receipt.cleanup_complete);
    assert_ne!(
        task_files::file_identity(&File::open(target).unwrap()).unwrap(),
        source_identity
    );
    assert!(publication.cleanup());
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 1);
}

#[test]
fn 输出必须有明确的文件名和已存在父目录() {
    let root = tempfile::tempdir().unwrap();
    assert!(matches!(
        Publication::new(&root.path().join("output.txt")),
        Err(PublicationError::OutputInvalid)
    ));
    assert!(matches!(
        Publication::new(&root.path().join("不存在的父目录/output.apk")),
        Err(PublicationError::OutputChanged)
    ));
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 0);
}

#[test]
fn 已有文件目录及软硬链接全部拒绝且原文件不变() {
    let root = tempfile::tempdir().unwrap();
    let original = root.path().join("原文件.apk");
    fs::write(&original, CONTENT).unwrap();
    let directory = root.path().join("目录.apk");
    fs::create_dir(&directory).unwrap();
    let hard = root.path().join("硬链接.apk");
    fs::hard_link(&original, &hard).unwrap();
    let soft = root.path().join("软链接.apk");
    symlink(&original, &soft).unwrap();
    let dangling = root.path().join("悬空链接.apk");
    symlink(root.path().join("不存在"), &dangling).unwrap();
    for path in [&original, &directory, &hard, &soft, &dangling] {
        assert!(matches!(
            Publication::new(path),
            Err(PublicationError::OutputExists)
        ));
    }
    assert_eq!(fs::read(&original).unwrap(), CONTENT);
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 5);
}

#[test]
fn 错误摘要失败且清理只涉及本次目录() {
    let root = tempfile::tempdir().unwrap();
    let unrelated = root.path().join("其他任务.txt");
    fs::write(&unrelated, "必须保留".as_bytes()).unwrap();
    let mut publication = prepared(root.path());
    let target = publication.target.clone();
    assert!(matches!(
        publication.publish_after_stop([0; 32], |_| Ok(())),
        Err(PublicationError::DigestMismatch)
    ));
    assert!(publication.cleanup());
    assert!(!target.exists());
    assert_eq!(fs::read(unrelated).unwrap(), "必须保留".as_bytes());
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 1);
}

#[test]
fn 缺失空文件目录及链接产物不能发布() {
    #[cfg(unix)]
    let kinds = [
        "missing",
        "empty",
        "directory",
        "symlink",
        "hardlink",
        "fifo",
    ];
    #[cfg(windows)]
    let kinds = ["missing", "empty", "directory", "symlink", "hardlink"];
    for kind in kinds {
        let root = tempfile::tempdir().unwrap();
        let mut publication = Publication::new(&root.path().join("output.apk")).unwrap();
        let candidate = publication.work_dir().unwrap().join("signed.apk");
        let original = root.path().join("外部材料");
        fs::write(&original, CONTENT).unwrap();
        match kind {
            "empty" => fs::write(&candidate, b"").unwrap(),
            "directory" => fs::create_dir(&candidate).unwrap(),
            "symlink" => symlink(&original, &candidate).unwrap(),
            "hardlink" => fs::hard_link(&original, &candidate).unwrap(),
            #[cfg(unix)]
            "fifo" => assert!(shield_core::utils::no_window_command("mkfifo")
                .arg(&candidate)
                .status()
                .unwrap()
                .success()),
            _ => {}
        }
        assert!(
            matches!(
                publication.publish_after_stop(digest(CONTENT), |_| Ok(())),
                Err(PublicationError::InvalidArtifact)
            ),
            "{kind}"
        );
        assert!(publication.cleanup());
        assert!(!root.path().join("output.apk").exists());
        assert_eq!(fs::read(&original).unwrap(), CONTENT);
    }
}

#[test]
fn 复制期间取消或超时均不发布半成品() {
    for interrupted in [PublicationError::Cancelled, PublicationError::TimedOut] {
        let root = tempfile::tempdir().unwrap();
        let mut publication = prepared(root.path());
        let bytes = vec![7; 192 * 1024];
        fs::write(publication.work_dir().unwrap().join("signed.apk"), &bytes).unwrap();
        let mut checkpoints = 0;
        let result = publication.publish_after_stop(digest(&bytes), |_| {
            checkpoints += 1;
            if checkpoints == 3 {
                Err(interrupted)
            } else {
                Ok(())
            }
        });
        assert!(matches!(result, Err(error) if error == interrupted));
        assert!(publication.cleanup());
        assert_eq!(fs::read_dir(root.path()).unwrap().count(), 0);
    }
}

#[test]
fn 复制完成后最后检查取消仍不发布() {
    let root = tempfile::tempdir().unwrap();
    let mut publication = prepared(root.path());
    let mut checkpoints = 0;
    let result = publication.publish_after_stop(digest(CONTENT), |_| {
        checkpoints += 1;
        if checkpoints == 4 {
            Err(PublicationError::Cancelled)
        } else {
            Ok(())
        }
    });
    assert!(matches!(result, Err(PublicationError::Cancelled)));
    assert!(publication.cleanup());
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 0);
}

#[test]
fn 固化后修改工作产物不会改变最终字节() {
    let root = tempfile::tempdir().unwrap();
    let mut publication = prepared(root.path());
    let source = publication.work_dir().unwrap().join("signed.apk");
    let mut checkpoints = 0;
    let receipt = publication
        .publish_after_stop(digest(CONTENT), |_| {
            checkpoints += 1;
            if checkpoints == 4 {
                fs::write(&source, b"changed after sealing").unwrap();
            }
            Ok(())
        })
        .unwrap();
    assert_eq!(receipt.sha256, digest(CONTENT));
    assert_eq!(fs::read(&publication.target).unwrap(), CONTENT);
    assert!(receipt.cleanup_complete);
}

#[test]
fn 准备后出现输出不得覆盖() {
    let root = tempfile::tempdir().unwrap();
    let mut publication = prepared(root.path());
    fs::write(&publication.target, "别人刚创建的文件".as_bytes()).unwrap();
    assert!(matches!(
        publication.publish_after_stop(digest(CONTENT), |_| Ok(())),
        Err(PublicationError::OutputExists)
    ));
    assert!(publication.cleanup());
    assert_eq!(
        fs::read(&publication.target).unwrap(),
        "别人刚创建的文件".as_bytes()
    );
}

#[test]
fn 两个并发任务只有一个能够提交() {
    for _ in 0..12 {
        let root = tempfile::tempdir().unwrap();
        let first = prepared(root.path());
        let second = prepared(root.path());
        let barrier = Arc::new(Barrier::new(2));
        let handles: Vec<_> = [first, second]
            .into_iter()
            .map(|mut publication| {
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    barrier.wait();
                    let result = publication.publish_after_stop(digest(CONTENT), |_| Ok(()));
                    assert!(publication.cleanup());
                    result
                })
            })
            .collect();
        let mut successes = 0;
        for handle in handles {
            match handle.join().unwrap() {
                Ok(receipt) => {
                    successes += 1;
                    assert!(receipt.location_confirmed);
                }
                Err(PublicationError::OutputExists) => {}
                _ => panic!("出现非预期提交状态"),
            }
        }
        assert_eq!(successes, 1);
        assert_eq!(
            fs::read(root.path().join("最终中文输出 空格.apk")).unwrap(),
            CONTENT
        );
        assert_eq!(fs::read_dir(root.path()).unwrap().count(), 1);
    }
}

#[test]
#[cfg(unix)]
fn 父目录链接改向拒绝且不写入新目录() {
    let root = tempfile::tempdir().unwrap();
    let first = root.path().join("原目录");
    let second = root.path().join("新目录");
    fs::create_dir(&first).unwrap();
    fs::create_dir(&second).unwrap();
    let link = root.path().join("输出位置");
    symlink(&first, &link).unwrap();
    let mut publication = prepared(&link);
    fs::remove_file(&link).unwrap();
    symlink(&second, &link).unwrap();
    assert!(matches!(
        publication.publish_after_stop(digest(CONTENT), |_| Ok(())),
        Err(PublicationError::OutputChanged)
    ));
    assert!(publication.cleanup());
    assert_eq!(fs::read_dir(&first).unwrap().count(), 0);
    assert_eq!(fs::read_dir(&second).unwrap().count(), 0);
}

#[test]
#[cfg(unix)]
fn 父目录同路径换成新目录也拒绝() {
    let root = tempfile::tempdir().unwrap();
    let parent = root.path().join("输出");
    fs::create_dir(&parent).unwrap();
    let mut publication = prepared(&parent);
    let moved = root.path().join("原目录已移走");
    fs::rename(&parent, &moved).unwrap();
    fs::create_dir(&parent).unwrap();
    assert!(matches!(
        publication.publish_after_stop(digest(CONTENT), |_| Ok(())),
        Err(PublicationError::OutputChanged)
    ));
    assert!(!publication.cleanup());
    assert!(!publication.cleanup());
    assert_eq!(fs::read_dir(&parent).unwrap().count(), 0);
    assert_eq!(fs::read_dir(&moved).unwrap().count(), 1);
}

#[test]
#[cfg(unix)]
fn 暂存目录被替换后不递归删除替代目录() {
    let root = tempfile::tempdir().unwrap();
    let mut publication = prepared(root.path());
    let work = publication.work_dir().unwrap().to_owned();
    fs::rename(&work, root.path().join("被移动的本次暂存")).unwrap();
    fs::create_dir(&work).unwrap();
    fs::write(work.join("不可删除.txt"), CONTENT).unwrap();
    assert!(matches!(
        publication.publish_after_stop(digest(CONTENT), |_| Ok(())),
        Err(PublicationError::OutputChanged)
    ));
    assert!(!publication.cleanup());
    assert!(!publication.cleanup());
    assert_eq!(fs::read(work.join("不可删除.txt")).unwrap(), CONTENT);
    assert!(!publication.target.exists());
}

#[test]
#[cfg(unix)]
#[ignore = "需要普通用户权限及支持目录权限的本地文件系统，显式验证提交后清理失败"]
fn 提交后清理失败不能误报没有输出() {
    let root = tempfile::tempdir().unwrap();
    let mut publication = prepared(root.path());
    let work = publication.work_dir().unwrap().to_owned();
    let mut checkpoints = 0;
    let result = publication.publish_after_stop(digest(CONTENT), |_| {
        checkpoints += 1;
        if checkpoints == 4 {
            fs::set_permissions(&work, fs::Permissions::from_mode(0o500)).unwrap();
        }
        Ok(())
    });
    // 无论断言结果怎样，先恢复本次临时目录权限，保证测试夹具可以回收。
    if work.exists() {
        fs::set_permissions(&work, fs::Permissions::from_mode(0o700)).unwrap();
    }
    let receipt = result.unwrap();
    assert!(
        !receipt.cleanup_complete,
        "请使用非 Root 用户和支持目录权限的文件系统运行该专项"
    );
    assert!(!publication.cleanup());
    assert_eq!(fs::read(&publication.target).unwrap(), CONTENT);
}

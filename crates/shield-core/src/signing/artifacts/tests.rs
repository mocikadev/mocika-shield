use super::*;

fn prepared(root: &Path, v4: bool) -> Artifacts {
    let artifacts = Artifacts::new(&root.join("输出.apk"), v4).unwrap();
    fs::write(artifacts.staged_apk(), b"new-apk").unwrap();
    if v4 {
        fs::write(sidecar(&artifacts.staged_apk()), b"new-idsig").unwrap();
    }
    artifacts
}

fn original(root: &Path) {
    fs::write(root.join("输出.apk"), b"old-apk").unwrap();
    fs::write(root.join("输出.apk.idsig"), b"old-idsig").unwrap();
}

#[test]
fn 成功覆盖同时提交附件且关闭_v4_清理旧附件() {
    let root = tempfile::tempdir().unwrap();
    original(root.path());
    prepared(root.path(), true).publish().unwrap();
    assert_eq!(fs::read(root.path().join("输出.apk")).unwrap(), b"new-apk");
    assert_eq!(
        fs::read(root.path().join("输出.apk.idsig")).unwrap(),
        b"new-idsig"
    );
    prepared(root.path(), false).publish().unwrap();
    assert!(!root.path().join("输出.apk.idsig").exists());
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 1);
}

#[test]
fn 提交前失败取消和缺失附件均保留旧输出() {
    let root = tempfile::tempdir().unwrap();
    original(root.path());
    let artifacts = prepared(root.path(), true);
    let stage = artifacts.directory.path().to_owned();
    drop(artifacts);
    assert!(!stage.exists());
    let artifacts = prepared(root.path(), false);
    let missing = Artifacts {
        v4: true,
        ..artifacts
    };
    assert!(missing.publish().is_err());
    assert_eq!(fs::read(root.path().join("输出.apk")).unwrap(), b"old-apk");
    assert_eq!(
        fs::read(root.path().join("输出.apk.idsig")).unwrap(),
        b"old-idsig"
    );
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 2);
}

#[test]
fn 每个提交步骤失败均恢复旧产物且不留残件() {
    for failing_step in 1..=4 {
        let root = tempfile::tempdir().unwrap();
        original(root.path());
        let artifacts = prepared(root.path(), true);
        let mut calls = 0;
        assert!(artifacts
            .publish_with(|from, to| {
                calls += 1;
                if calls == failing_step {
                    Err(std::io::ErrorKind::PermissionDenied.into())
                } else {
                    fs::rename(from, to)
                }
            })
            .is_err());
        assert_eq!(fs::read(root.path().join("输出.apk")).unwrap(), b"old-apk");
        assert_eq!(
            fs::read(root.path().join("输出.apk.idsig")).unwrap(),
            b"old-idsig"
        );
        assert_eq!(fs::read_dir(root.path()).unwrap().count(), 2);
    }
}

#[test]
#[cfg(windows)]
fn 旧输出或附件被占用时恢复原产物且不留残件() {
    use std::os::windows::fs::OpenOptionsExt;

    for name in ["输出.apk", "输出.apk.idsig"] {
        let root = tempfile::tempdir().unwrap();
        original(root.path());
        let artifacts = prepared(root.path(), true);
        let occupied = fs::OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(root.path().join(name))
            .unwrap();
        let result = artifacts.publish();
        drop(occupied);

        assert!(result.is_err(), "文件独占期间不能替换旧产物");
        assert_eq!(fs::read(root.path().join("输出.apk")).unwrap(), b"old-apk");
        assert_eq!(
            fs::read(root.path().join("输出.apk.idsig")).unwrap(),
            b"old-idsig"
        );
        assert_eq!(fs::read_dir(root.path()).unwrap().count(), 2);
    }
}

#[test]
fn 回滚也失败时保留恢复材料不销毁原文件() {
    let root = tempfile::tempdir().unwrap();
    original(root.path());
    let artifacts = prepared(root.path(), true);
    let stage = artifacts.directory.path().to_owned();
    let mut calls = 0;
    let result = artifacts.publish_with(|from, to| {
        calls += 1;
        if calls >= 4 {
            Err(std::io::ErrorKind::PermissionDenied.into())
        } else {
            fs::rename(from, to)
        }
    });
    assert!(result.unwrap_err().to_string().contains("恢复材料已保留"));
    assert_eq!(fs::read(stage.join("previous.apk")).unwrap(), b"old-apk");
    assert_eq!(
        fs::read(stage.join("previous.idsig")).unwrap(),
        b"old-idsig"
    );
}

#[test]
fn 新建输出提交失败不会遗留孤立附件() {
    let root = tempfile::tempdir().unwrap();
    let artifacts = prepared(root.path(), true);
    let output = artifacts.output.clone();
    let result = artifacts.publish_with(|from, to| {
        if to == output {
            Err(std::io::ErrorKind::PermissionDenied.into())
        } else {
            fs::rename(from, to)
        }
    });
    assert!(result.is_err());
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 0);
}

#[test]
fn 目录冲突或空产物不能替换旧输出() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join("输出.apk")).unwrap();
    assert!(Artifacts::new(&root.path().join("输出.apk"), false).is_err());
    let other = root.path().join("另一个.apk");
    fs::create_dir(sidecar(&other)).unwrap();
    assert!(Artifacts::new(&other, false).is_err());
    let artifacts = Artifacts::new(&root.path().join("空.apk"), false).unwrap();
    fs::write(artifacts.staged_apk(), []).unwrap();
    assert!(artifacts.publish().is_err());
}

#[test]
#[cfg(unix)]
fn 输出或附件为链接时拒绝且不修改指向文件() {
    let root = tempfile::tempdir().unwrap();
    let original = root.path().join("原件.apk");
    fs::write(&original, b"original").unwrap();
    let output = root.path().join("链接.apk");
    std::os::unix::fs::symlink(&original, &output).unwrap();
    assert!(Artifacts::new(&output, false).is_err());
    let output = root.path().join("附件链接.apk");
    std::os::unix::fs::symlink(&original, sidecar(&output)).unwrap();
    assert!(Artifacts::new(&output, true).is_err());
    assert_eq!(fs::read(original).unwrap(), b"original");
}

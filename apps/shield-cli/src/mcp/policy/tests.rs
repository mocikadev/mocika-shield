use super::*;
use std::os::unix::fs::symlink;

#[test]
fn 共享范围须显式白名单且不创建缺失资料或固定整库摘要() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("输入");
    let output = dir.path().join("输出");
    fs::create_dir(&input).unwrap();
    fs::create_dir(&output).unwrap();
    let cli = dir.path().join("shield");
    fs::write(&cli, b"fixture").unwrap();
    let home = dir.path().join("应用资料");
    let make = || Policy::new(&input, &output, None, &cli, 10).unwrap();
    assert!(make().with_shared(Some(&home), vec![]).is_err());
    assert!(make()
        .with_shared(Some(&home), vec!["same".into(), "same".into()])
        .is_err());
    let policy = make()
        .with_shared(Some(&home), vec!["allowed".into()])
        .unwrap();
    assert!(!home.exists());
    assert!(policy.check().is_ok());
    assert!(policy.certificate("allowed").is_ok());
    assert_eq!(policy.certificate("other"), Err("CERTIFICATE_NOT_ALLOWED"));
    fs::create_dir_all(home.join("data")).unwrap();
    fs::write(home.join("data/shield.db"), b"state 1").unwrap();
    assert!(policy.check().is_ok());
    fs::write(home.join("data/shield.db"), b"state 2 unrelated changes").unwrap();
    assert!(policy.check().is_ok());
    fs::remove_file(home.join("data/shield.db")).unwrap();
    symlink(&cli, home.join("data/shield.db")).unwrap();
    assert_eq!(policy.check(), Err("SHARED_CONTEXT_CHANGED"));
    assert!(make()
        .with_shared(Some(&output), vec!["allowed".into()])
        .is_err());
}

#[test]
fn 路径只接受根目录内普通文件且保留中文空格() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("中文 文件.apk"), b"apk").unwrap();
    let root = Root::open(dir.path()).unwrap();
    assert!(root.resolve("中文 文件.apk", false).is_ok());
    for name in [
        "", "/tmp/a", "../a", "a/../b", "./a", "a//b", "a/", "a\nb", ".",
    ] {
        assert!(root.resolve(name, false).is_err(), "{name:?}");
    }
    assert!(root.resolve("新 输出.apk", true).is_ok());
    assert_eq!(
        root.resolve("中文 文件.apk", true).unwrap_err(),
        "OUTPUT_EXISTS"
    );
    assert!(root.resolve("缺目录/a.apk", true).is_err());
}

#[test]
fn 链接和悬空链接均拒绝且不借相似前缀越界() {
    let dir = tempfile::tempdir().unwrap();
    let sibling = tempfile::tempdir().unwrap();
    fs::write(sibling.path().join("a.apk"), b"apk").unwrap();
    symlink(sibling.path(), dir.path().join("逃逸")).unwrap();
    symlink(sibling.path().join("a.apk"), dir.path().join("链接.apk")).unwrap();
    symlink(sibling.path().join("不存在"), dir.path().join("悬空.apk")).unwrap();
    let root = Root::open(dir.path()).unwrap();
    for name in ["逃逸/a.apk", "链接.apk", "悬空.apk"] {
        assert_eq!(root.resolve(name, false).unwrap_err(), "PATH_NOT_ALLOWED");
        assert_eq!(root.resolve(name, true).unwrap_err(), "PATH_NOT_ALLOWED");
    }
}

#[test]
fn 根目录和可信文件替换后失败关闭() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("根");
    fs::create_dir(&path).unwrap();
    let root = Root::open(&path).unwrap();
    fs::rename(&path, dir.path().join("旧根")).unwrap();
    fs::create_dir(&path).unwrap();
    assert_eq!(root.check(), Err("ROOT_CHANGED"));
    let config = dir.path().join("shield-cli.toml");
    fs::write(&config, b"schema_version = 1").unwrap();
    let pinned = PinnedFile::open(&config, 1024).unwrap();
    assert!(pinned.check(1024).is_ok());
    fs::write(&config, b"schema_version = 2").unwrap();
    assert_eq!(pinned.check(1024), Err("TRUSTED_FILE_CHANGED"));
    assert!(Root::open(Path::new("/")).is_err());
    assert!(Root::open(Path::new("relative")).is_err());
}

#[test]
fn 配置链接保留查找位置但改向后拒绝且输出不能进入应用包() {
    let dir = tempfile::tempdir().unwrap();
    let original = dir.path().join("原配置");
    let replacement = dir.path().join("替换配置");
    fs::write(&original, b"schema_version=1").unwrap();
    fs::write(&replacement, b"schema_version=1").unwrap();
    let link = dir.path().join("shield-cli.toml");
    symlink(&original, &link).unwrap();
    let pinned = PinnedFile::open(&link, 1024).unwrap();
    assert_eq!(pinned.path, link);
    fs::remove_file(&link).unwrap();
    symlink(&replacement, &link).unwrap();
    assert_eq!(pinned.check(1024), Err("TRUSTED_FILE_CHANGED"));
    fs::create_dir(dir.path().join("嵌套.APP")).unwrap();
    let root = Root::open(dir.path()).unwrap();
    assert_eq!(
        root.resolve("嵌套.APP/a.apk", true).unwrap_err(),
        "PATH_NOT_ALLOWED"
    );
}

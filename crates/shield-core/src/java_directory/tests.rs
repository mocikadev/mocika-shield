use super::*;

#[test]
fn 非ascii缺失不同目录及原长名不能充当安全别名() {
    let root = tempfile::tempdir().unwrap();
    let other = tempfile::tempdir().unwrap();
    let pinned = open_directory(root.path()).unwrap();
    assert!(validate_alias(root.path(), &pinned, other.path().to_owned()).is_err());
    assert!(validate_alias(root.path(), &pinned, root.path().join("中文")).is_err());
    assert!(validate_alias(root.path(), &pinned, root.path().join("missing")).is_err());
    // 不依赖卷配置，模拟短名 API 返回未改写的长名。
    let unicode = root.path().join("中文");
    std::fs::create_dir(&unicode).unwrap();
    let unicode_pin = open_directory(&unicode).unwrap();
    assert!(validate_alias(&unicode, &unicode_pin, unicode.clone()).is_err());
    let file = root.path().join("file");
    std::fs::write(&file, b"not a directory").unwrap();
    assert!(ascii_directory(&file).is_err());
    assert!(ascii_directory(Path::new("relative")).is_err());
}

#[test]
fn 保留前缀设备路径控制字符和无法表示的路径拒绝() {
    for path in [
        r"\\?\C:\dir",
        r"\\?\UNC\server\share",
        r"\\.\C:\dir",
        "C:\\dir\n",
        "C:\\dir\"",
        "C:\\dir\0",
    ] {
        assert!(checked(Path::new(path)).is_err());
    }
    let invalid_utf16 = OsString::from_wide(&[b'C' as u16, b':' as u16, b'\\' as u16, 0xd800]);
    assert!(checked(Path::new(&invalid_utf16)).is_err());
    assert!(checked(Path::new(r"\\server\share\ascii space")).is_ok());
}

#[test]
fn 已有短名保持真实身份且被替换后拒绝() {
    let root = tempfile::tempdir().unwrap();
    let directory = root.path().join("中文 补充😀");
    std::fs::create_dir(&directory).unwrap();
    let pinned = open_directory(&directory).unwrap();
    // 当前原生验收卷须提供短名；不允许把换成 ASCII 样本当作兼容通过。
    let alias = ascii_directory(&directory).expect("当前验收卷没有安全短名，路径门槛未通过");
    assert!(alias.as_os_str().is_ascii());
    assert!(same_directory(&directory, &alias).unwrap());
    assert!(matches_directory(&alias, &pinned).unwrap());
    let moved = root.path().join("moved");
    std::fs::rename(&directory, &moved).unwrap();
    std::fs::create_dir(&directory).unwrap();
    assert!(!matches_directory(&directory, &pinned).unwrap());
    assert!(validate_alias(&directory, &pinned, alias).is_err());
}

#[test]
fn 长路径可核对身份但不能直接交给java() {
    let root = tempfile::tempdir().unwrap();
    let mut native = std::fs::canonicalize(root.path()).unwrap();
    for _ in 0..12 {
        native.push("long-directory-name");
        std::fs::create_dir(&native).unwrap();
    }
    native.push("中文😀");
    std::fs::create_dir(&native).unwrap();
    let pinned = open_directory(&native).unwrap();
    assert!(matches_directory(&native, &pinned).unwrap());
    match ascii_directory(&native) {
        Ok(alias) => {
            assert!(checked(&alias).is_ok());
            assert!(matches_directory(&alias, &pinned).unwrap());
        }
        Err(_) => {
            // 未承诺所有长路径均有可供旧 Java 使用的安全短名。
            assert!(matches_directory(&native, &pinned).unwrap());
        }
    }
}

#[test]
fn 目录联接不能代替原生普通目录() {
    let root = tempfile::tempdir().unwrap();
    let target = root.path().join("target");
    let junction = root.path().join("junction");
    std::fs::create_dir(&target).unwrap();
    // 仅在本次临时目录创建联接，不要求启用系统开发者模式或调整权限。
    let output = crate::utils::no_window_command("cmd.exe")
        .args(["/D", "/C", "mklink", "/J"])
        .arg(&junction)
        .arg(&target)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "临时联接负向夹具未建立，不能计为通过"
    );
    assert!(open_directory(&junction).is_err());
    assert!(ascii_directory(&junction).is_err());
    assert!(same_directory(&target, &junction).is_err());
    assert!(target.is_dir());
    std::fs::remove_dir(&junction).unwrap();
}

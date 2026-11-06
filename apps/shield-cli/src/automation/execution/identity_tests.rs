use super::*;
use std::fs;
use std::os::unix::fs::symlink;

fn executable(root: &Path, name: &str) -> PathBuf {
    let path = root.join(name);
    std::fs::write(&path, b"local executable bytes").unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
    path
}

#[test]
fn 内容或同字节文件被替换都拒绝() {
    for same_bytes in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let path = executable(root.path(), "executor");
        let identity = Executable::capture("executor", &path, &mut || Ok(()))
            .ok()
            .unwrap();
        let replacement = executable(root.path(), "replacement");
        if !same_bytes {
            fs::write(&replacement, b"different bytes").unwrap();
        }
        fs::rename(replacement, &path).unwrap();
        assert!(identity.quick(&mut || Ok(())).is_err());
        assert!(matches!(
            identity.recheck(&mut || Ok(())),
            Err(CheckError {
                code: "EXECUTABLE_CHANGED",
                ..
            })
        ));
    }
}

#[test]
fn 链接改向权限改变及非普通文件都拒绝() {
    let root = tempfile::tempdir().unwrap();
    let first = executable(root.path(), "first");
    let second = executable(root.path(), "second");
    let link = root.path().join("selected");
    symlink(&first, &link).unwrap();
    let identity = Executable::capture("executor", &link, &mut || Ok(()))
        .ok()
        .unwrap();
    fs::remove_file(&link).unwrap();
    symlink(second, &link).unwrap();
    assert!(identity.recheck(&mut || Ok(())).is_err());
    assert!(identity.quick(&mut || Ok(())).is_err());
    let identity = Executable::capture("java", &first, &mut || Ok(()))
        .ok()
        .unwrap();
    fs::set_permissions(first, fs::Permissions::from_mode(0o600)).unwrap();
    assert!(identity.recheck(&mut || Ok(())).is_err());
    assert!(identity.quick(&mut || Ok(())).is_err());
    assert!(Executable::capture("executor", root.path(), &mut || Ok(())).is_err());
}

#[test]
fn 取消和期限原样传播且不读取文件() {
    for code in ["TASK_CANCELLED", "TASK_TIMEOUT"] {
        let outcome =
            Executable::capture("executor", Path::new("不存在"), &mut || Err(error(code)));
        assert!(matches!(outcome, Err(CheckError { code: actual, .. }) if actual == code));
    }
}

#[test]
fn 正常可执行身份可复核且私有描述不接受额外字段() {
    let root = tempfile::tempdir().unwrap();
    let path = executable(root.path(), "executor");
    let identity = Executable::capture("executor", &path, &mut || Ok(()))
        .ok()
        .unwrap();
    assert!(identity.recheck(&mut || Ok(())).is_ok());
    let mut value = serde_json::to_value(&identity).unwrap();
    value["command"] = serde_json::json!("不允许的扩展");
    assert!(serde_json::from_value::<Executable>(value).is_err());
}

#[test]
fn 工具优先路径只设置到本次子进程并保留其原有路径() {
    let root = tempfile::tempdir().unwrap();
    let java = executable(root.path(), "java");
    let keytool = executable(root.path(), "keytool");
    let tools = Tools {
        java: Executable::capture("java", &java, &mut || Ok(()))
            .ok()
            .unwrap(),
        keytool: Executable::capture("keytool", &keytool, &mut || Ok(()))
            .ok()
            .unwrap(),
    };
    let original = std::env::var_os("PATH");
    let mut command = shield_core::utils::no_window_command("未启动的测试命令");
    command.env("PATH", "/fixture/original");
    tools.pin(&mut command).ok().unwrap();
    let path = command
        .get_envs()
        .find(|(key, _)| *key == "PATH")
        .unwrap()
        .1
        .unwrap();
    let paths: Vec<_> = std::env::split_paths(path).collect();
    assert_eq!(
        paths,
        [root.path().to_owned(), PathBuf::from("/fixture/original")]
    );
    assert_eq!(std::env::var_os("PATH"), original);
    assert!(tools.recheck(&mut || Ok(())).is_ok());
}

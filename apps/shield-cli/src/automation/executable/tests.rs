use super::*;
#[cfg(unix)]
use std::os::unix::fs::{symlink, PermissionsExt};
#[cfg(windows)]
use std::os::windows::fs::symlink_file as symlink;
use std::{fs, path::PathBuf};

fn executable(root: &Path, name: &str) -> PathBuf {
    #[cfg(unix)]
    let path = root.join(name);
    #[cfg(windows)]
    let path = root.join(format!("{name}.exe"));
    std::fs::write(&path, b"local executable bytes").unwrap();
    #[cfg(unix)]
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
    #[cfg(unix)]
    fs::set_permissions(&first, fs::Permissions::from_mode(0o600)).unwrap();
    #[cfg(windows)]
    let original_permissions = fs::metadata(&first).unwrap().permissions();
    #[cfg(windows)]
    {
        let mut permissions = original_permissions.clone();
        permissions.set_readonly(true);
        fs::set_permissions(&first, permissions).unwrap();
    }
    assert!(identity.recheck(&mut || Ok(())).is_err());
    assert!(identity.quick(&mut || Ok(())).is_err());
    #[cfg(windows)]
    {
        // 仅恢复本次夹具的原属性，避免临时目录因只读文件无法回收。
        fs::set_permissions(&first, original_permissions).unwrap();
    }
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
    let inherited = root.path().join("原有工具路径");
    command.env(if cfg!(windows) { "Path" } else { "PATH" }, &inherited);
    tools.pin(&mut command).ok().unwrap();
    let path = command
        .get_envs()
        .find(|(key, _)| path_key(key))
        .unwrap()
        .1
        .unwrap();
    let paths: Vec<_> = std::env::split_paths(path).collect();
    assert_eq!(
        paths,
        [shield_core::utils::strip_unc_prefix(root.path()), inherited]
    );
    assert_eq!(std::env::var_os("PATH"), original);
    assert!(tools.recheck(&mut || Ok(())).is_ok());
}

#[test]
fn 当前执行器角色路径及序列化身份复核() {
    let path = std::env::current_exe().unwrap();
    let identity = Executable::capture("executor", &path, &mut || Ok(()))
        .ok()
        .unwrap();
    assert!(identity.worker().is_ok());
    assert!(identity.selected(&path, &mut || Ok(())).is_ok());
    assert!(identity
        .selected(&path.with_file_name("other.exe"), &mut || Ok(()))
        .is_err());
    let encoded = serde_json::to_vec(&identity).unwrap();
    let decoded: Executable = serde_json::from_slice(&encoded).unwrap();
    assert!(identity == decoded);
    assert!(decoded.recheck(&mut || Ok(())).is_ok());
    let mut value = serde_json::to_value(&identity).unwrap();
    value["stamp"]["extra"] = serde_json::json!(true);
    assert!(serde_json::from_value::<Executable>(value).is_err());
    let tool = Executable::capture("java", &path, &mut || Ok(()))
        .ok()
        .unwrap();
    assert!(tool.worker().is_err());
}

#[test]
fn 计划来源和独立文件入口共用同一工具身份且取消不改报() {
    let root = tempfile::tempdir().unwrap();
    let dir = root.path().join("中文 工具");
    fs::create_dir(&dir).unwrap();
    let java = executable(&dir, "java");
    let keytool = executable(&dir, "keytool");
    let java = snapshot::file("java", &java).ok().unwrap();
    let keytool = snapshot::file("keytool", &keytool).ok().unwrap();
    let sources = Sources {
        config: None,
        #[cfg(all(
            feature = "agent-executor",
            any(target_os = "macos", target_os = "linux", windows)
        ))]
        shared: None,
        input: java.clone(),
        keystore: java.clone(),
        apktool: java.clone(),
        apksigner: java.clone(),
        resources: java.clone(),
        #[cfg(all(
            feature = "agent-executor",
            any(target_os = "macos", target_os = "linux", windows)
        ))]
        java_adapter: None,
        java,
        keytool,
    };
    let tools = Tools::capture(&sources, &mut || Ok(())).ok().unwrap();
    assert!(tools.matches(&sources).is_ok());
    assert!(Tools::capture_files(&sources.java, &sources.keytool, &mut || Ok(())).is_ok());
    assert!(tools
        .matches_files(&sources.keytool, &sources.java)
        .is_err());
    for code in ["TASK_CANCELLED", "TASK_TIMEOUT"] {
        assert!(
            matches!(Tools::capture(&sources, &mut || Err(error(code))), Err(CheckError { code: actual, .. }) if actual == code)
        );
    }
    fs::write(&sources.java.path, b"changed executable").unwrap();
    assert!(matches!(
        tools.recheck(&mut || Ok(())),
        Err(CheckError {
            code: "TOOLCHAIN_CHANGED",
            ..
        })
    ));
}

#[cfg(windows)]
#[test]
fn 不把批处理脚本或普通目录当原生可执行文件() {
    let root = tempfile::tempdir().unwrap();
    for name in ["java.cmd", "keytool.bat", "shield.txt"] {
        let path = root.path().join(name);
        fs::write(&path, b"not an exe").unwrap();
        assert!(Executable::capture("java", &path, &mut || Ok(())).is_err());
    }
    let path = root.path().join("directory.exe");
    fs::create_dir(&path).unwrap();
    assert!(Executable::capture("java", &path, &mut || Ok(())).is_err());
}

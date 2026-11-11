//! Linux／Windows 安装布局的原生子进程回归；仅验证依赖发现，不冒充安装包或 APK 业务验收。
#![cfg(any(
    all(target_os = "macos", feature = "mcp-server"),
    target_os = "linux",
    windows
))]
use serde_json::Value;
use std::fs;
use std::path::Path;

#[cfg(any(target_os = "linux", windows))]
fn inspect(binary: &Path, work: &Path) -> Value {
    let output = shield_core::utils::no_window_command(binary)
        .current_dir(work)
        .args(["doctor", "--json"])
        .output()
        .unwrap();
    assert!(output.stderr.is_empty(), "不得输出工具原文");
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["type"], "result");
    assert_eq!(value["operation"], "doctor");
    value
}

#[test]
#[cfg(any(target_os = "linux", windows))]
fn 桌面命令在包外工作且缺同包资源不回退源码() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("中文 安装路径");
    let work = directory.path().join("其他 工作路径");
    fs::create_dir_all(&work).unwrap();
    #[cfg(windows)]
    let (binary, resources) = (root.join("mocika-shield-cli.exe"), root.clone());
    #[cfg(target_os = "linux")]
    let (binary, resources) = (
        root.join("usr/bin/mocika-shield-cli"),
        root.join("usr/lib/MocikaShield"),
    );
    fs::create_dir_all(binary.parent().unwrap()).unwrap();
    fs::copy(env!("CARGO_BIN_EXE_shield"), &binary).unwrap();
    // doctor 只核对资源存在和可读；不把合成文件当成有效工具运行。
    for relative in [
        "tools/apktool.jar",
        "tools/apksigner.jar",
        "resources/resources.zip",
    ] {
        let path = resources.join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, "仅验证资源发现").unwrap();
    }
    let before = inspect(&binary, &work);
    for dependency in ["apktool", "apksigner", "resources"] {
        let checks = before["data"]["checks"].as_array().unwrap();
        let check = checks
            .iter()
            .find(|check| check["dependency"] == dependency)
            .unwrap();
        assert_eq!(check["status"], "ready");
    }
    fs::remove_file(resources.join("tools/apktool.jar")).unwrap();
    let missing = inspect(&binary, Path::new(env!("CARGO_MANIFEST_DIR")));
    let checks = missing["data"]["checks"].as_array().unwrap();
    assert_eq!(
        checks
            .iter()
            .find(|check| check["dependency"] == "apktool")
            .unwrap()["status"],
        "blocked"
    );
    #[cfg(feature = "mcp-server")]
    for args in [
        ["agent", "--help"].as_slice(),
        ["mcp", "serve", "--help"].as_slice(),
    ] {
        assert!(shield_core::utils::no_window_command(&binary)
            .current_dir(&work)
            .args(args)
            .output()
            .unwrap()
            .status
            .success());
    }
}

#[cfg(feature = "mcp-server")]
#[path = "desktop_bundle/installed.rs"]
mod installed;

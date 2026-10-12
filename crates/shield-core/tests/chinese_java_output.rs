//! 用真实 JDK 的非 UTF-8 默认输出验证错误路径，不模拟工具响应。

#[test]
#[ignore = "需要 SHIELD_TEST_JDK_HOME 指定真实 JDK"]
fn 中文路径参数及双输出流保持完整() {
    use shield_core::utils::{java_command, no_window_command};
    use std::path::PathBuf;

    let home = PathBuf::from(std::env::var_os("SHIELD_TEST_JDK_HOME").expect("请指定真实 JDK"));
    let bin = |name: &str| {
        home.join("bin").join(if cfg!(windows) {
            format!("{name}.exe")
        } else {
            name.to_string()
        })
    };
    let dir = tempfile::tempdir().unwrap();
    let classes = dir.path().join("中文类目录 空格");
    std::fs::create_dir(&classes).unwrap();
    let path = classes.join("中文应用 空格&符号.apk");
    std::fs::write(&path, "中文内容完整").unwrap();
    let source = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/java-toolchain/ChineseIoProbe.java");
    let compile = no_window_command(bin("javac"))
        .args(["-encoding", "UTF-8", "-d"])
        .arg(&classes)
        .arg(source)
        .output()
        .unwrap();
    assert!(compile.status.success(), "Java 探针编译失败");
    for encoding in ["GBK", "US-ASCII"] {
        let output = java_command(bin("java"))
            .arg("-cp").arg(&classes).arg("ChineseIoProbe").arg(&path)
            .env("JAVA_TOOL_OPTIONS", format!("-Dfile.encoding={encoding} -Dsun.stdout.encoding={encoding} -Dsun.stderr.encoding={encoding}"))
            .env_remove("_JAVA_OPTIONS").env_remove("JDK_JAVA_OPTIONS")
            .output().unwrap();
        assert!(output.status.success(), "中文路径文件读取失败");
        let stdout = std::str::from_utf8(&output.stdout).expect("标准输出不是 UTF-8");
        let stderr = std::str::from_utf8(&output.stderr).expect("标准错误不是 UTF-8");
        assert!(stdout.contains("中文应用 空格&符号.apk"));
        assert!(stdout.contains("中文内容完整"));
        assert!(stderr.contains("中文错误详情："));
        assert!(stderr.contains("中文应用 空格&符号.apk"));
    }
}

#[test]
#[ignore = "需要 SHIELD_TEST_JDK_HOME 指定真实 JDK"]
fn 中文_apk_错误路径在非_utf8环境保持完整() {
    use shield_core::utils::no_window_command;
    use std::path::PathBuf;

    let home = PathBuf::from(std::env::var_os("SHIELD_TEST_JDK_HOME").expect("请指定真实 JDK"));
    if std::env::var_os("SHIELD_CHINESE_OUTPUT_CHILD").is_some() {
        let dir = tempfile::tempdir().unwrap();
        let apk = dir.path().join("中文应用 空格.apk");
        let signer = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tools/apksigner.jar");
        let error = shield_core::extract_apk_cert_fingerprint(&apk, Some(&signer)).unwrap_err();
        let text = error.to_string();
        assert!(
            text.contains("中文应用 空格.apk"),
            "错误路径发生乱码：{text}"
        );
        assert!(!text.contains('\u{fffd}'), "错误消息包含解码替代字符");
        return;
    }
    for encoding in ["GBK", "US-ASCII"] {
        let output = no_window_command(std::env::current_exe().unwrap())
            .args(["--ignored", "--exact", "中文_apk_错误路径在非_utf8环境保持完整", "--nocapture"])
            .env("SHIELD_CHINESE_OUTPUT_CHILD", "1")
            .env("JAVA_HOME", &home)
            .env("PATH", home.join("bin"))
            .env("JAVA_TOOL_OPTIONS", format!("-Dfile.encoding={encoding} -Dsun.stdout.encoding={encoding} -Dsun.stderr.encoding={encoding}"))
            .env_remove("_JAVA_OPTIONS")
            .env_remove("JDK_JAVA_OPTIONS")
            .output().unwrap();
        assert!(
            output.status.success(),
            "编码 {encoding} 回归失败：\n{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

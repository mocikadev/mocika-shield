//! 显式提供本机 JDK 后运行；子进程隔离环境变量，不污染并行测试。

#[cfg(unix)]
#[test]
#[ignore = "需要通过 SHIELD_TEST_JDK_HOME 指定本机真实 JDK"]
fn real_jdk_discovery_and_certificate_reading() {
    use shield_core::keytool::keytool_command;
    use shield_core::utils::{no_window_command, probe_java_environment};
    use std::os::unix::fs::PermissionsExt;
    use std::path::PathBuf;

    let home = PathBuf::from(std::env::var_os("SHIELD_TEST_JDK_HOME").expect("请指定真实 JDK"));
    if std::env::var_os("SHIELD_TEST_JDK_CHILD").is_some() {
        let info = probe_java_environment();
        assert!(info.java_ready(), "Java 未就绪：{info:?}");
        assert!(info.keytool_ready(), "keytool 未就绪：{info:?}");
        let selected = std::fs::canonicalize(info.keytool_path.as_ref().unwrap()).unwrap();
        let same_installation = [home.join("bin/keytool"), home.join("jre/bin/keytool")]
            .iter()
            .filter_map(|path| std::fs::canonicalize(path).ok())
            .any(|path| path == selected);
        assert!(same_installation, "keytool 不属于所选 JDK");
        let temp = tempfile::tempdir().unwrap();
        let certificate = temp.path().join("中文证书 空格.jks");
        let output = keytool_command(shield_core::utils::find_keytool().unwrap())
            .args([
                "-genkeypair",
                "-alias",
                "中文别名",
                "-keyalg",
                "RSA",
                "-keysize",
                "2048",
                "-validity",
                "1",
                "-dname",
                "CN=测试",
                "-storetype",
                "JKS",
                "-storepass",
                "fixture-password",
                "-keypass",
                "fixture-password",
                "-keystore",
            ])
            .arg(&certificate)
            .output()
            .unwrap();
        assert!(output.status.success(), "测试证书生成失败");
        let fingerprint = shield_core::extract_keystore_cert_fingerprint(
            &certificate,
            "中文别名",
            "fixture-password",
            Some("JKS"),
        )
        .unwrap();
        assert_eq!(fingerprint.len(), 64);
        println!(
            "真实 JDK 验证通过：Java {}，探测、生成证书、中文 Alias 指纹读取正常",
            info.version_label()
        );
        return;
    }

    let temp = tempfile::tempdir().unwrap();
    let bin = temp.path().join("中文 启动目录");
    std::fs::create_dir(&bin).unwrap();
    let java = bin.join("java");
    std::os::unix::fs::symlink(home.join("bin/java"), &java).unwrap();
    for mode in ["symlink", "java_home", "launcher"] {
        if mode == "launcher" {
            std::fs::remove_file(&java).unwrap();
            let program = home
                .join("bin/java")
                .display()
                .to_string()
                .replace('\'', "'\\''");
            std::fs::write(&java, format!("#!/bin/sh\nexec '{program}' \"$@\"\n")).unwrap();
            std::fs::set_permissions(&java, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let mut command = no_window_command(std::env::current_exe().unwrap());
        command
            .args([
                "--ignored",
                "--exact",
                "real_jdk_discovery_and_certificate_reading",
                "--nocapture",
            ])
            .env("SHIELD_TEST_JDK_CHILD", "1")
            .env(
                "PATH",
                if mode == "java_home" {
                    temp.path()
                } else {
                    &bin
                },
            )
            .env_remove("JAVA_HOME");
        if mode == "java_home" {
            command.env("JAVA_HOME", &home);
        }
        let output = command.output().unwrap();
        assert!(
            output.status.success(),
            "场景 {mode} 失败：\n{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        println!("场景 {mode}：{}", String::from_utf8_lossy(&output.stdout));
    }
}

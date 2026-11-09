//! 使用隔离子进程验证只读协议；模拟 Java 仅验证编排，不证明真实 APK 签名有效。
use serde_json::Value;
use std::process::{Command, Output};

fn command() -> Command {
    let mut command = shield_core::utils::no_window_command(env!("CARGO_BIN_EXE_shield"));
    command.env_remove("JAVA_HOME").env("NO_COLOR", "1");
    command
}

#[test]
#[cfg(not(feature = "mcp-server"))]
fn 默认构建不开放本地协议服务() {
    let output = command().args(["mcp", "serve"]).output().unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
}

fn response(output: &Output, exit: i32, operation: &str) -> Value {
    assert_eq!(output.status.code(), Some(exit), "{output:?}");
    assert!(output.stderr.is_empty(), "{output:?}");
    let value: Value =
        serde_json::from_slice(&output.stdout).expect("标准输出应只有一个 JSON 对象");
    assert_eq!(value["schema_version"], 1);
    assert_eq!(value["tool_version"], env!("CARGO_PKG_VERSION"));
    assert_eq!(value["operation"], operation);
    value
}

#[test]
fn 新检查命令不静默忽略旧配置() {
    for operation in ["doctor", "preflight"] {
        let mut cmd = command();
        cmd.args(["--config", "不存在的配置.toml", operation]);
        if operation == "preflight" {
            cmd.args(["-i", "不存在.apk"]);
        }
        let output = cmd.output().unwrap();
        let value = response(&output, 1, operation);
        assert_eq!(value["type"], "error");
        assert_eq!(value["code"], "CONFIG_NOT_SUPPORTED");
        assert!(!String::from_utf8_lossy(&output.stdout).contains("不存在的配置"));
    }
}

#[test]
fn 缺少输入时返回有限错误而不暴露路径() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("私密应用.apk");
    let output = command()
        .arg("preflight")
        .arg("-i")
        .arg(&path)
        .output()
        .unwrap();
    let value = response(&output, 1, "preflight");
    assert_eq!(value["code"], "FILE_NOT_FOUND");
    assert!(!String::from_utf8_lossy(&output.stdout).contains("私密应用"));
}

#[test]
fn 目录不能被当成输入文件() {
    let temp = tempfile::tempdir().unwrap();
    let output = command()
        .arg("preflight")
        .arg("-i")
        .arg(temp.path())
        .output()
        .unwrap();
    let value = response(&output, 1, "preflight");
    assert_eq!(value["code"], "INPUT_NOT_FILE");
}

#[test]
fn 非法指纹与未知模式属于语法错误() {
    for args in [
        vec!["--expected-cert-sha256", "AA:BB"],
        vec!["--runtime-profile", "随意降级"],
    ] {
        let output = command()
            .args(["preflight", "-i", "input.apk"])
            .args(args)
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
        assert!(!output.stderr.is_empty());
    }
}

#[test]
fn 旧加固签名错误事件保持不变() {
    for name in ["protect", "sign"] {
        let output = command()
            .args([name, "--json"])
            .env_remove("MOCIKA_SHIELD_KS_PASS")
            .env_remove("MOCIKA_SHIELD_KEY_PASS")
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(1));
        let value: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(value["type"], "error");
        assert!(value["message"].is_string());
        assert!(value.get("schema_version").is_none());
    }
}

#[test]
fn 帮助不是检查结果() {
    let output = command().args(["doctor", "--help"]).output().unwrap();
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("--operation"));
}

#[cfg(not(all(feature = "agent-executor", target_os = "macos")))]
#[test]
fn 默认构建不开放内部执行入口() {
    for name in ["agent", "__agent-probe", "__agent-worker"] {
        let output = command().args([name, "--help"]).output().unwrap();
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
    }
}

#[test]
fn 不静默忽略与操作无关的工具参数() {
    for args in [["sign", "--resources"], ["certificate", "--apksigner"]] {
        let output = command()
            .args(["doctor", "--operation", args[0], args[1], "任意路径"])
            .output()
            .unwrap();
        let value = response(&output, 1, "doctor");
        assert_eq!(value["code"], "ARGUMENT_NOT_APPLICABLE");
    }
}

#[test]
#[ignore = "需先构建真实冒烟 APK，并在本机安装完整 JDK"]
fn 真实签名冒烟包只读预检() {
    use shield_core::keytool::keytool_command;
    use shield_core::utils::{find_java, find_keytool, java_command};
    use std::fs;
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let unsigned = root.join(
        "tests/fixtures/android-smoke-app/app/build/outputs/apk/release/app-release-unsigned.apk",
    );
    let signer = root.join("tools/apksigner.jar");
    assert!(unsigned.is_file(), "请先构建冒烟样本，不下载业务 APK 代替");
    let temp = tempfile::tempdir().unwrap();
    let keystore = temp.path().join("临时证书.p12");
    let signed = temp.path().join("真实中文应用 空格.apk");
    let original = fs::read(&unsigned).unwrap();
    let result = keytool_command(find_keytool().unwrap())
        .args([
            "-genkeypair",
            "-noprompt",
            "-storetype",
            "PKCS12",
            "-alias",
            "smoke",
            "-keyalg",
            "RSA",
            "-keysize",
            "2048",
            "-validity",
            "1",
            "-dname",
            "CN=自动化检查临时证书",
            "-storepass",
            "fixture-only-password",
            "-keypass",
            "fixture-only-password",
            "-keystore",
        ])
        .arg(&keystore)
        .output()
        .unwrap();
    assert!(result.status.success(), "创建临时测试证书失败");
    let result = java_command(find_java().unwrap())
        .arg("-jar")
        .arg(&signer)
        .args(["sign", "--ks"])
        .arg(&keystore)
        .args([
            "--ks-pass",
            "env:SHIELD_FIXTURE_PASS",
            "--ks-key-alias",
            "smoke",
            "--out",
        ])
        .arg(&signed)
        .arg(&unsigned)
        .env("SHIELD_FIXTURE_PASS", "fixture-only-password")
        .output()
        .unwrap();
    assert!(result.status.success(), "签名临时测试 APK 失败");
    let before = fs::read(&signed).unwrap();
    let fingerprint = shield_core::extract_apk_cert_fingerprint(&signed, Some(&signer)).unwrap();
    let output = command()
        .arg("preflight")
        .arg("-i")
        .arg(&signed)
        .arg("--apksigner")
        .arg(&signer)
        .arg("--expected-cert-sha256")
        .arg(fingerprint)
        .output()
        .unwrap();
    let value = response(&output, 0, "preflight");
    assert_eq!(value["data"]["certificate_binding_checked"], true);
    assert!(value["data"]["checks"]
        .as_array()
        .unwrap()
        .iter()
        .any(|c| c["code"] == "certificate" && c["status"] == "ready"));
    let config = temp.path().join("shield-cli.toml");
    fs::write(
        &config,
        r#"schema_version = 1
[sign]
keystore = "临时证书.p12"
key_alias = "smoke"
keystore_type = "pkcs12"
"#,
    )
    .unwrap();
    let planned = temp.path().join("计划输出.apk");
    let output = command()
        .arg("--config")
        .arg(&config)
        .arg("plan-protect")
        .arg("-i")
        .arg(&signed)
        .arg("-o")
        .arg(&planned)
        .env("MOCIKA_SHIELD_KS_PASS", "fixture-only-password")
        .env_remove("MOCIKA_SHIELD_KEY_PASS")
        .output()
        .unwrap();
    let value = response(&output, 0, "plan-protect");
    assert_eq!(value["data"]["execution_ready"], false);
    assert_eq!(value["data"]["resource_protocol_checked"], true);
    assert_eq!(value["data"]["plan_schema_version"], 2);
    assert!(value["data"]["plan_digest"].is_string());
    let standard_digest = value["data"]["plan_digest"].clone();
    let output = command()
        .arg("--config")
        .arg(&config)
        .arg("plan-protect")
        .arg("-i")
        .arg(&signed)
        .arg("-o")
        .arg(&planned)
        .args(["--runtime-profile", "android-api19"])
        .env("MOCIKA_SHIELD_KS_PASS", "fixture-only-password")
        .env_remove("MOCIKA_SHIELD_KEY_PASS")
        .output()
        .unwrap();
    let value = response(&output, 0, "plan-protect");
    assert_eq!(value["data"]["resource_protocol_checked"], true);
    assert_eq!(value["data"]["runtime_profile"], "android-api19");
    assert_ne!(value["data"]["plan_digest"], standard_digest);
    assert!(!planned.exists());
    // 真实独立签名覆盖核心 apksigner 环境引用，不以模拟工具代替签名有效性。
    let resigned = temp.path().join("独立重签名.apk");
    let output = command()
        .arg("--config")
        .arg(&config)
        .arg("sign")
        .arg("-i")
        .arg(&signed)
        .arg("-o")
        .arg(&resigned)
        .arg("--json")
        .env("MOCIKA_SHIELD_KS_PASS", "fixture-only-password")
        .env_remove("MOCIKA_SHIELD_KEY_PASS")
        .output()
        .unwrap();
    assert!(output.status.success(), "真实 CLI 独立签名失败");
    assert!(output.stderr.is_empty());
    let events: Vec<Value> = String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(events.last().unwrap()["type"], "done");
    assert_eq!(
        shield_core::extract_apk_cert_fingerprint(&signed, Some(&signer)).unwrap(),
        shield_core::extract_apk_cert_fingerprint(&resigned, Some(&signer)).unwrap()
    );
    assert_eq!(before, fs::read(&signed).unwrap());
    assert_eq!(original, fs::read(&unsigned).unwrap());
}

#[cfg(unix)]
mod isolated_java {
    use super::*;
    use std::fs;
    use std::io::Write;
    use std::os::unix::fs::PermissionsExt;
    use std::path::Path;

    fn java(root: &Path, version: &str, verify_exit: i32) -> Command {
        let bin = root.join("隔离 Java 空格");
        fs::create_dir_all(&bin).unwrap();
        let java = bin.join("java");
        fs::write(&java, format!(r#"#!/bin/sh
for argument in "$@"; do
    if [ "$argument" = "-version" ]; then
        echo 'openjdk version "{version}"' >&2
        exit 0
    fi
done
echo '不可泄露的工具原始输出'
echo '不可泄露的工具原始错误' >&2
echo 'Signer #1 certificate SHA-256 digest: aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa'
exit {verify_exit}
"#)).unwrap();
        fs::set_permissions(&java, fs::Permissions::from_mode(0o700)).unwrap();
        command_with_path(&bin)
    }

    fn command_with_path(path: &Path) -> Command {
        let mut cmd = command();
        cmd.env("PATH", path);
        cmd
    }

    fn file(root: &Path, name: &str) -> std::path::PathBuf {
        let path = root.join(name);
        fs::write(&path, "仅验证文件可读，不是真实资源").unwrap();
        path
    }

    fn apk(root: &Path, abis: &[&str]) -> std::path::PathBuf {
        let path = root.join("中文应用 空格.apk");
        let mut zip = zip::ZipWriter::new(fs::File::create(&path).unwrap());
        for name in std::iter::once("AndroidManifest.xml".to_owned())
            .chain(std::iter::once("classes.dex".to_owned()))
            .chain(abis.iter().map(|abi| format!("lib/{abi}/libtest.so")))
        {
            zip.start_file(name, zip::write::SimpleFileOptions::default())
                .unwrap();
            zip.write_all("编排夹具，不是可安装 APK".as_bytes())
                .unwrap();
        }
        zip.finish().unwrap();
        path
    }

    fn preflight(root: &Path, path: &Path, verify_exit: i32) -> Command {
        let mut cmd = java(root, "17.0.1", verify_exit);
        cmd.arg("preflight")
            .arg("-i")
            .arg(path)
            .arg("--apksigner")
            .arg(file(root, "签名工具.jar"));
        cmd
    }

    #[test]
    fn 缺少_java_返回明确阻断结果() {
        let temp = tempfile::tempdir().unwrap();
        let output = command_with_path(temp.path())
            .args(["doctor", "--operation", "certificate"])
            .output()
            .unwrap();
        let value = response(&output, 1, "doctor");
        assert_eq!(value["status"], "blocked");
        assert_eq!(value["data"]["checks"][0]["code"], "JAVA_NOT_FOUND");
    }

    #[test]
    fn 独立签名不要求_keytool_且不把文件可读夸成完整验证() {
        let temp = tempfile::tempdir().unwrap();
        let jar = file(temp.path(), "签名工具.jar");
        let output = java(temp.path(), "1.8.0_504", 0)
            .args(["doctor", "--json", "--operation", "sign", "--apksigner"])
            .arg(&jar)
            .output()
            .unwrap();
        let value = response(&output, 0, "doctor");
        assert_eq!(value["status"], "ready");
        assert_eq!(value["data"]["scope"], "dependency_presence");
        assert_eq!(value["data"]["java_major"], 8);
        let checks = value["data"]["checks"].as_array().unwrap();
        assert_eq!(checks.len(), 2);
        assert_eq!(checks[1]["verification"], "readable_file");
        let output = java(temp.path(), "17.0.1", 0)
            .args(["doctor", "--operation", "certificate"])
            .output()
            .unwrap();
        assert_eq!(response(&output, 1, "doctor")["status"], "blocked");
    }

    #[test]
    fn 低版本和未知_java_分别报告() {
        for (version, code) in [
            ("1.7.0_80", "JAVA_UNSUPPORTED"),
            ("unknown", "JAVA_PROBE_FAILED"),
        ] {
            let temp = tempfile::tempdir().unwrap();
            let output = java(temp.path(), version, 0)
                .args(["doctor", "--operation", "sign", "--apksigner"])
                .arg(file(temp.path(), "签名.jar"))
                .output()
                .unwrap();
            let value = response(&output, 1, "doctor");
            assert_eq!(value["data"]["checks"][0]["code"], code);
        }
    }

    #[test]
    fn 显式失效工具不回退且目录不算工具() {
        let temp = tempfile::tempdir().unwrap();
        for path in [temp.path().join("缺失.jar"), temp.path().to_path_buf()] {
            let output = java(temp.path(), "17.0.1", 0)
                .args(["doctor", "--operation", "sign", "--apksigner"])
                .arg(path)
                .output()
                .unwrap();
            let value = response(&output, 1, "doctor");
            assert_eq!(value["data"]["checks"][1]["status"], "blocked");
        }
    }

    #[test]
    fn 加固依赖检查不要求_keytool() {
        let temp = tempfile::tempdir().unwrap();
        let jar = file(temp.path(), "工具.jar");
        let resources = file(temp.path(), "资源.zip");
        let output = java(temp.path(), "17.0.1", 0)
            .arg("doctor")
            .arg("--apktool")
            .arg(&jar)
            .arg("--apksigner")
            .arg(&jar)
            .arg("--resources")
            .arg(resources)
            .output()
            .unwrap();
        let value = response(&output, 0, "doctor");
        assert_eq!(value["data"]["checks"].as_array().unwrap().len(), 4);
    }

    #[test]
    fn 标准与工控规则不同且输入保持不变() {
        let temp = tempfile::tempdir().unwrap();
        let input = apk(temp.path(), &["arm64-v8a"]);
        let before = fs::read(&input).unwrap();
        let value = response(
            &preflight(temp.path(), &input, 0).output().unwrap(),
            0,
            "preflight",
        );
        assert_eq!(value["status"], "ready");
        assert_eq!(value["data"]["certificate_binding_checked"], false);
        assert_eq!(
            value["data"]["facts"]["native_abis"],
            serde_json::json!(["arm64-v8a"])
        );
        let output = preflight(temp.path(), &input, 0)
            .args(["--runtime-profile", "android-api19"])
            .output()
            .unwrap();
        let value = response(&output, 1, "preflight");
        assert_eq!(value["status"], "blocked");
        assert!(value["data"]["checks"]
            .as_array()
            .unwrap()
            .iter()
            .any(|c| c["code"] == "runtime_api19_target" && c["status"] == "blocked"));
        assert_eq!(before, fs::read(&input).unwrap());
    }

    #[test]
    fn 未支持架构警告不自动授权排除() {
        let temp = tempfile::tempdir().unwrap();
        let input = apk(temp.path(), &["armeabi-v7a", "armeabi"]);
        let output = preflight(temp.path(), &input, 0).output().unwrap();
        let value = response(&output, 0, "preflight");
        assert_eq!(value["status"], "warning");
        assert_eq!(
            value["data"]["facts"]["native_abis"],
            serde_json::json!(["armeabi", "armeabi-v7a"])
        );
    }

    #[test]
    fn 验签失败阻断且原始输出不穿透协议() {
        let temp = tempfile::tempdir().unwrap();
        let input = apk(temp.path(), &[]);
        let output = preflight(temp.path(), &input, 1).output().unwrap();
        let value = response(&output, 1, "preflight");
        assert_eq!(value["status"], "blocked");
        assert!(value["data"]["checks"]
            .as_array()
            .unwrap()
            .iter()
            .any(|check| check["code"] == "signature_not_verified"));
        assert!(!String::from_utf8_lossy(&output.stdout).contains("不可泄露"));
    }

    #[test]
    fn 预检缺少工具时不进入签名条目回退() {
        let temp = tempfile::tempdir().unwrap();
        let input = apk(temp.path(), &[]);
        let output = command_with_path(temp.path())
            .arg("preflight")
            .arg("-i")
            .arg(&input)
            .output()
            .unwrap();
        let value = response(&output, 1, "preflight");
        assert_eq!(value["code"], "TOOL_NOT_FOUND");
        let output = java(temp.path(), "17.0.1", 0)
            .arg("preflight")
            .arg("-i")
            .arg(&input)
            .arg("--apksigner")
            .arg(temp.path().join("缺失签名工具.jar"))
            .output()
            .unwrap();
        let value = response(&output, 1, "preflight");
        assert_eq!(value["type"], "error");
        assert_eq!(value["code"], "FILE_NOT_FOUND");
    }

    #[test]
    fn 指纹匹配与不匹配均返回明确结果() {
        let temp = tempfile::tempdir().unwrap();
        let input = apk(temp.path(), &[]);
        for (fingerprint, exit, code) in [
            ("aa:".repeat(31) + "aa", 0, "certificate"),
            ("bb".repeat(32), 1, "certificate_mismatch"),
        ] {
            let output = preflight(temp.path(), &input, 0)
                .arg("--expected-cert-sha256")
                .arg(fingerprint)
                .output()
                .unwrap();
            let value = response(&output, exit, "preflight");
            assert_eq!(value["data"]["certificate_binding_checked"], true);
            assert!(value["data"]["checks"]
                .as_array()
                .unwrap()
                .iter()
                .any(|c| c["code"] == code));
        }
    }

    #[test]
    fn 损坏归档返回固定错误不暴露工具文本() {
        let temp = tempfile::tempdir().unwrap();
        let input = file(temp.path(), "损坏的业务包.apk");
        let output = preflight(temp.path(), &input, 0).output().unwrap();
        let value = response(&output, 1, "preflight");
        assert_eq!(value["type"], "error");
        assert!(value["code"].is_string());
        assert!(!String::from_utf8_lossy(&output.stdout).contains("损坏的业务包"));
    }

    #[test]
    fn 超长架构名称不截断成通过结果() {
        let temp = tempfile::tempdir().unwrap();
        let abi = "a".repeat(65);
        let input = apk(temp.path(), &["arm64-v8a", &abi]);
        let output = preflight(temp.path(), &input, 0).output().unwrap();
        let value = response(&output, 1, "preflight");
        assert_eq!(value["code"], "INPUT_LIMIT_EXCEEDED");
        assert!(!String::from_utf8_lossy(&output.stdout).contains(&abi));
    }
}

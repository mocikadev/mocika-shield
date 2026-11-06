//! 开发特性专用闭环；临时证书只用于测试副本，不能视为业务签名或设备兼容验收。
#![cfg(all(feature = "agent-executor", target_os = "macos"))]
use serde_json::{json, Value};
use std::fs;
use std::io::Write;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::Path;
use std::process::{Command, Output};

#[path = "support/execution_fixture.rs"]
mod fixture;
use fixture::hash;

fn command() -> Command {
    let mut command = shield_core::utils::no_window_command(env!("CARGO_BIN_EXE_shield"));
    command
        .env_remove("MOCIKA_SHIELD_KS_PASS")
        .env_remove("MOCIKA_SHIELD_KEY_PASS");
    command
}
fn private(path: &Path, content: &[u8]) {
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .unwrap();
    file.write_all(content).unwrap();
}
fn response(output: Output) -> Value {
    assert!(output.stderr.is_empty(), "内部错误不得回传原文");
    serde_json::from_slice(&output.stdout).unwrap()
}
fn request(root: &Path, config: &Path, input: &Path, output: &Path) -> std::path::PathBuf {
    let path = root.join("request.json");
    private(&path, &serde_json::to_vec(&json!({ "config":config,"input":input,"output":output,"runtime_profile":"standard","exclude_abis":[],"timeout_seconds":1200 })).unwrap());
    path
}

#[test]
fn 明确拒绝第四代签名且在无运行时环境中也不静默降级() {
    let root = tempfile::tempdir().unwrap();
    let config = root.path().join("shield-cli.toml");
    private(
        &config,
        b"schema_version=1\n[sign]\nkeystore='missing.p12'\nkey_alias='fixture'\nv4=true\n",
    );
    let request = request(
        root.path(),
        &config,
        &root.path().join("missing.apk"),
        &root.path().join("output.apk"),
    );
    let output = command()
        .args(["__agent-probe", "--request"])
        .arg(request)
        .env("PATH", "")
        .env_remove("JAVA_HOME")
        .env("MOCIKA_SHIELD_KS_PASS", "fixture-only-password")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(response(output)["code"], "SIGNING_V4_NOT_SUPPORTED");
    assert!(!root.path().join("output.apk").exists());
}

#[test]
fn 既有输出与密码字段都在启动工作之前拒绝() {
    let root = tempfile::tempdir().unwrap();
    let output_path = root.path().join("existing.apk");
    fs::write(&output_path, b"keep").unwrap();
    let path = request(
        root.path(),
        &root.path().join("none.toml"),
        &root.path().join("input.apk"),
        &output_path,
    );
    let output = command()
        .args(["__agent-probe", "--request"])
        .arg(&path)
        .output()
        .unwrap();
    assert_eq!(response(output)["code"], "OUTPUT_EXISTS");
    assert_eq!(fs::read(&output_path).unwrap(), b"keep");
    fs::remove_file(&path).unwrap();
    private(&path, br#"{"password":"must-not-be-accepted"}"#);
    let output = command()
        .args(["__agent-probe", "--request"])
        .arg(path)
        .output()
        .unwrap();
    let value = response(output);
    assert_eq!(value["code"], "PRIVATE_FILE_INVALID");
    assert!(!value.to_string().contains("must-not-be-accepted"));
}

#[test]
fn 准备阶段工具挂起会真实超时且无最终输出() {
    let root = tempfile::tempdir().unwrap();
    let jdk = root.path().join("jdk");
    fs::create_dir_all(jdk.join("bin")).unwrap();
    let java = jdk.join("bin/java");
    private(&java, b"#!/bin/sh\ncase \" $* \" in *' -version '*) echo 'openjdk version \"17.0.1\"' >&2;; *) exec /bin/sleep 30;; esac\n");
    let keytool = jdk.join("bin/keytool");
    private(&keytool, b"#!/bin/sh\nexec /bin/sleep 30\n");
    for path in [&java, &keytool] {
        fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
    }
    for name in ["input.apk", "fixture.p12", "tool.jar", "resources.zip"] {
        private(&root.path().join(name), b"local fixture");
    }
    let config = root.path().join("shield-cli.toml");
    private(&config, b"schema_version=1\n[protect]\napktool='tool.jar'\napksigner='tool.jar'\nresources='resources.zip'\n[sign]\nkeystore='fixture.p12'\nkey_alias='fixture'\n");
    let output_path = root.path().join("output.apk");
    let path = request(
        root.path(),
        &config,
        &root.path().join("input.apk"),
        &output_path,
    );
    let mut value: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    value["timeout_seconds"] = json!(1);
    fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
    let started = std::time::Instant::now();
    let output = command()
        .args(["__agent-probe", "--request"])
        .arg(path)
        .env("PATH", jdk.join("bin"))
        .env("JAVA_HOME", jdk)
        .env("MOCIKA_SHIELD_KS_PASS", "fixture-only-password")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    let value = response(output);
    assert_eq!(value["code"], "TASK_TIMEOUT");
    assert_eq!(value["status"], "timed_out");
    assert!(started.elapsed() < std::time::Duration::from_secs(8));
    assert!(!output_path.exists());
    for entry in fs::read_dir(root.path()).unwrap() {
        assert!(!entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".mocika-"));
    }
}

#[test]
#[ignore = "需要本机 JDK、资源和 SHIELD_AGENT_TEST_APK，显式运行真实 APK 工具闭环，不纳入日常 CI"]
fn 真实安装包临时证书副本隔离加固签名与无覆盖交付() {
    let fixture = fixture::Fixture::new();
    let root = fixture.root();
    let original = &fixture.original;
    let signed_input = &fixture.input;
    let output = &fixture.output;
    let signer = &fixture.signer;
    let path = request(root, &fixture.config, signed_input, output);
    let result = fixture
        .command(Path::new(env!("CARGO_BIN_EXE_shield")))
        .args(["__agent-probe", "--request"])
        .arg(&path)
        .output()
        .unwrap();
    let code = result.status.code();
    let value = response(result);
    assert_eq!(code, Some(0), "有限闭环结果：{value}");
    assert_eq!(value["status"], "succeeded", "有限闭环结果：{value}");
    assert_eq!(value["output_state"], "published");
    assert_eq!(value["cleanup_complete"], true);
    assert!(output.is_file());
    assert_eq!(
        value["artifact"]["sha256"],
        hash(output)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    );
    fixture.assert_inputs_unchanged();
    let zipalign = std::env::var_os("SHIELD_AGENT_TEST_ZIPALIGN")
        .map(std::path::PathBuf::from)
        .expect("请指定 Android SDK zipalign 作独立对齐核对");
    assert!(
        shield_core::utils::no_window_command(zipalign)
            .args(["-c", "-P", "16", "4"])
            .arg(output)
            .output()
            .unwrap()
            .status
            .success(),
        "官方 zipalign 对齐检查未通过"
    );
    let input_certificate =
        shield_core::extract_apk_cert_fingerprint(signed_input, Some(signer)).unwrap();
    let output_certificate =
        shield_core::extract_apk_cert_fingerprint(output, Some(signer)).unwrap();
    assert_eq!(input_certificate, output_certificate);
    fixture.assert_no_tasks();
    eprintln!("真实 APK 隔离闭环通过：输入 {} 字节，输出 {} 字节，耗时 {} 毫秒；原始包及签名副本未变，临时产物随测试回收。", fs::metadata(original).unwrap().len(), fs::metadata(output).unwrap().len(), value["elapsed_ms"]);
}

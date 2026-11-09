//! 公共命令进程契约；轻量夹具不证明真实 APK 合格，真实工具专项另行显式运行。
#![cfg(all(feature = "agent-executor", target_os = "macos"))]
use serde_json::Value;
use std::fs;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, Instant};

#[path = "support/execution_fixture.rs"]
mod real_fixture;

fn command() -> Command {
    let mut command = shield_core::utils::no_window_command(env!("CARGO_BIN_EXE_shield"));
    command
        .env_remove("JAVA_HOME")
        .env_remove("JAVA_TOOL_OPTIONS")
        .env_remove("_JAVA_OPTIONS")
        .env_remove("JDK_JAVA_OPTIONS")
        .env("MOCIKA_SHIELD_KS_PASS", "fixture-password-not-user-secret");
    command
}

fn frames(output: &Output, exit: i32) -> Vec<Value> {
    assert_eq!(output.status.code(), Some(exit));
    assert!(output.stderr.is_empty(), "运行结果不得回传原始标准错误");
    let values: Vec<Value> = std::str::from_utf8(&output.stdout)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(values.first().unwrap()["type"], "started");
    assert_eq!(values.last().unwrap()["type"], "result");
    assert_eq!(
        values
            .iter()
            .filter(|value| value["type"] == "result")
            .count(),
        1
    );
    for (sequence, value) in values.iter().enumerate() {
        assert_eq!(value["schema_version"], 1);
        assert_eq!(value["sequence"], sequence);
        assert_eq!(value["task_id"], values[0]["task_id"]);
    }
    assert_eq!(values[0]["task_id"].as_str().unwrap().len(), 32);
    values
}

struct Fixture {
    root: Option<tempfile::TempDir>,
    active: bool,
}
impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let bin = root.path().join("bin");
        fs::create_dir(&bin).unwrap();
        fs::write(
            bin.join("java"),
            b"#!/bin/sh\necho 'openjdk version \"17.0.1\"' >&2\n",
        )
        .unwrap();
        fs::write(bin.join("keytool"), b"#!/bin/sh\necho $$ > \"$SHIELD_FIXTURE_ROOT/keytool-pid\"\nwhile :; do printf x >> \"$SHIELD_FIXTURE_ROOT/heartbeat\"; /bin/sleep 0.02; done\n").unwrap();
        for name in ["java", "keytool"] {
            fs::set_permissions(bin.join(name), fs::Permissions::from_mode(0o700)).unwrap();
        }
        for name in ["input.apk", "fixture.p12", "tool.jar", "resources.zip"] {
            fs::write(root.path().join(name), b"fixture").unwrap();
        }
        fs::write(root.path().join("shield-cli.toml"), "schema_version=1\n[protect]\napktool='tool.jar'\napksigner='tool.jar'\nresources='resources.zip'\n[sign]\nkeystore='fixture.p12'\nkey_alias='fixture'\n").unwrap();
        Self {
            root: Some(root),
            active: false,
        }
    }
    fn root(&self) -> &Path {
        self.root.as_ref().unwrap().path()
    }
    fn command(&mut self) -> Command {
        self.command_with_timeout("10")
    }
    fn command_with_timeout(&mut self, timeout: &str) -> Command {
        self.active = true;
        let mut cmd = command();
        cmd.arg("--config")
            .arg(self.root().join("shield-cli.toml"))
            .args(["agent", "plan", "-i"])
            .arg(self.root().join("input.apk"))
            .arg("-o")
            .arg(self.root().join("output.apk"))
            .args(["--timeout-seconds", timeout])
            .env("PATH", self.root().join("bin"))
            .env("SHIELD_FIXTURE_ROOT", self.root());
        cmd
    }
    fn ready(&self) {
        let until = Instant::now() + Duration::from_secs(6);
        while !self
            .root()
            .join("heartbeat")
            .metadata()
            .is_ok_and(|metadata| metadata.len() > 0)
        {
            assert!(Instant::now() < until, "准备工具未进入预期挂起门槛");
            std::thread::sleep(Duration::from_millis(5));
        }
    }
    fn assert_clean(&mut self) {
        assert!(!self.root().join("output.apk").exists());
        assert_eq!(fs::read(self.root().join("input.apk")).unwrap(), b"fixture");
        for entry in fs::read_dir(self.root()).unwrap() {
            assert!(!entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".mocika-"));
        }
        if let Ok(before) = fs::read(self.root().join("heartbeat")) {
            std::thread::sleep(Duration::from_millis(80));
            assert_eq!(fs::read(self.root().join("heartbeat")).unwrap(), before);
        }
        self.active = false;
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        if self.active {
            if let Some(root) = self.root.take() {
                let _ = root.keep();
            }
        }
    }
}

struct Running(Option<Child>);
impl Running {
    fn spawn(command: &mut Command) -> Self {
        Self(Some(
            command
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap(),
        ))
    }
    fn child(&mut self) -> &mut Child {
        self.0.as_mut().unwrap()
    }
    fn finish(mut self) -> Output {
        self.0.take().unwrap().wait_with_output().unwrap()
    }
}
impl Drop for Running {
    fn drop(&mut self) {
        if let Some(child) = &mut self.0 {
            if child.try_wait().ok().flatten().is_none() {
                let _ = child.kill();
            }
            let _ = child.wait();
        }
    }
}

#[test]
fn 缺配置返回同一事件信封而不是自由错误文本() {
    let output = command()
        .args(["agent", "plan", "-i", "input.apk", "-o", "output.apk"])
        .output()
        .unwrap();
    let values = frames(&output, 1);
    assert_eq!(values.last().unwrap()["code"], "SIGNING_CONFIG_REQUIRED");
    assert_eq!(values.last().unwrap()["output_state"], "not_published");
}

#[test]
fn 执行必须提供完整计划摘要且不接受密码参数() {
    for extras in [
        vec![],
        vec!["--expect-plan-sha256", "bad"],
        vec!["--ks-pass", "not-accepted"],
    ] {
        let output = command()
            .args(["agent", "protect", "-i", "input.apk", "-o", "output.apk"])
            .args(extras)
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
    }
}

#[test]
fn 默认不读取标准输入且第四代签名明确拒绝() {
    let mut fixture = Fixture::new();
    fs::OpenOptions::new()
        .append(true)
        .open(fixture.root().join("shield-cli.toml"))
        .unwrap()
        .write_all(b"v4=true\n")
        .unwrap();
    let output = fixture.command().stdin(Stdio::null()).output().unwrap();
    assert_eq!(
        frames(&output, 1).last().unwrap()["code"],
        "SIGNING_V4_NOT_SUPPORTED"
    );
    fixture.assert_clean();
}

#[test]
fn 显式输入取消和断开都确认停止后退出() {
    for eof in [false, true] {
        let mut fixture = Fixture::new();
        let mut command = fixture.command();
        command.arg("--control-stdin").stdin(Stdio::piped());
        let mut running = Running::spawn(&mut command);
        fixture.ready();
        let mut stdin = running.child().stdin.take().unwrap();
        if !eof {
            stdin.write_all(b"{\"type\":\"cancel\"}\n").unwrap();
        }
        drop(stdin);
        let output = running.finish();
        let values = frames(&output, 130);
        assert_eq!(values.last().unwrap()["status"], "cancelled");
        fixture.assert_clean();
    }
}

#[test]
fn 非法控制内容不透传且按失败收尾() {
    let mut fixture = Fixture::new();
    let mut command = fixture.command();
    command.arg("--control-stdin").stdin(Stdio::piped());
    let mut running = Running::spawn(&mut command);
    fixture.ready();
    running
        .child()
        .stdin
        .take()
        .unwrap()
        .write_all(b"{\"type\":\"cancel\",\"password\":\"must-not-be-returned\"}\n")
        .unwrap();
    let output = running.finish();
    let values = frames(&output, 1);
    assert_eq!(values.last().unwrap()["code"], "CONTROL_INVALID");
    assert!(!String::from_utf8_lossy(&output.stdout).contains("must-not-be-returned"));
    fixture.assert_clean();
}

#[test]
fn 中断及终止信号经取消流程停止后代() {
    for signal in [libc::SIGINT, libc::SIGTERM] {
        let mut fixture = Fixture::new();
        let mut running = Running::spawn(&mut fixture.command());
        fixture.ready();
        assert!(running.child().try_wait().unwrap().is_none());
        assert_eq!(
            unsafe { libc::kill(running.child().id() as i32, signal) },
            0
        );
        let output = running.finish();
        let values = frames(&output, 130);
        assert_eq!(values.last().unwrap()["cancel_source"], "signal");
        fixture.assert_clean();
    }
}

#[test]
fn 受控规划超时不会遗留工作进程或最终文件() {
    let mut fixture = Fixture::new();
    let output = fixture.command_with_timeout("3").output().unwrap();
    assert_eq!(frames(&output, 124).last().unwrap()["status"], "timed_out");
    fixture.assert_clean();
}

#[test]
fn 共享规划取消断开及超时同样停止后代且不交付() {
    use shield_services::{
        certificates::{CertificateStoreState, CertificateUpsertInput},
        paths::ApplicationPaths,
    };
    for mode in ["cancel", "eof", "timeout"] {
        let mut fixture = Fixture::new();
        let home = fixture.root().join("共享上下文");
        let paths = ApplicationPaths::isolated(&home).unwrap();
        let store =
            CertificateStoreState::initialize(paths.certificate_database(), paths.keystore_dir())
                .unwrap();
        let input = CertificateUpsertInput {
            id: None,
            name: "挂起夹具".into(),
            source_type: "external".into(),
            keystore_path: String::new(),
            keystore_password: "synthetic-shared-gate-password".into(),
            key_alias: "fixture".into(),
            key_password: String::new(),
            ks_type: Some("PKCS12".into()),
            sign_v1: true,
            sign_v2: true,
            sign_v3: true,
            sign_v4: false,
            auto_sign_enabled: true,
            note: String::new(),
            set_as_default: true,
            copy_keystore_to_managed: false,
            managed_file_name: None,
        };
        store
            .save_certificate(
                &input,
                fixture.root().join("fixture.p12").to_str().unwrap(),
                "fixture",
                None,
            )
            .unwrap();
        fs::write(fixture.root().join("shield-cli.toml"),"schema_version=1\n[protect]\napktool='tool.jar'\napksigner='tool.jar'\nresources='resources.zip'\n").unwrap();
        let before = fs::read(paths.certificate_database()).unwrap();
        let mut command = fixture.command_with_timeout(if mode == "timeout" { "3" } else { "10" });
        command
            .env_remove("MOCIKA_SHIELD_KS_PASS")
            .env_remove("MOCIKA_SHIELD_KEY_PASS")
            .arg("--app-home")
            .arg(&home)
            .arg("--use-default-certificate");
        let output = if mode == "timeout" {
            command.output().unwrap()
        } else {
            command.arg("--control-stdin").stdin(Stdio::piped());
            let mut running = Running::spawn(&mut command);
            fixture.ready();
            let mut input = running.child().stdin.take().unwrap();
            if mode == "cancel" {
                input.write_all(b"{\"type\":\"cancel\"}\n").unwrap();
            }
            drop(input);
            running.finish()
        };
        let values = frames(&output, if mode == "timeout" { 124 } else { 130 });
        assert_eq!(values.last().unwrap()["output_state"], "not_published");
        assert!(!String::from_utf8_lossy(&output.stdout).contains("synthetic-shared-gate-password"));
        assert_eq!(fs::read(paths.certificate_database()).unwrap(), before);
        fixture.assert_clean();
    }
}

#[test]
fn 关闭结果读端会停止任务而不是继续在后台运行() {
    let mut fixture = Fixture::new();
    let mut running = Running::spawn(&mut fixture.command());
    fixture.ready();
    drop(running.child().stdout.take());
    // 没有新增进度时由任务本身的期限兜底；终态写入失败返回回执未知。
    let output = running.finish();
    assert_eq!(output.status.code(), Some(3));
    assert!(output.stderr.is_empty());
    fixture.assert_clean();
}

fn real_command(fixture: &real_fixture::Fixture, action: &str) -> Command {
    let mut command = fixture.command(Path::new(env!("CARGO_BIN_EXE_shield")));
    command
        .arg("--config")
        .arg(&fixture.config)
        .args(["agent", action, "-i"])
        .arg(&fixture.input)
        .arg("-o")
        .arg(&fixture.output);
    command
}

fn real_plan(fixture: &real_fixture::Fixture) -> String {
    let output = real_command(fixture, "plan").output().unwrap();
    let values = frames(&output, 0);
    let result = values.last().unwrap();
    assert!(matches!(
        result["status"].as_str(),
        Some("ready" | "warning")
    ));
    assert_eq!(result["output_state"], "not_published");
    assert_eq!(result["plan"]["private_key_access_checked"], false);
    assert!(!fixture.output.exists());
    fixture.assert_no_tasks();
    result["plan"]["sha256"].as_str().unwrap().to_owned()
}

#[test]
#[ignore = "需要本机 JDK、资源、授权小型 APK 和 SDK zipalign，显式验收新接口，不纳入日常 CI"]
fn 真实自动化规划匹配摘要后交付且配置漂移拒绝执行() {
    let fixture = real_fixture::Fixture::new();
    let digest = real_plan(&fixture);
    let output = real_command(&fixture, "protect")
        .args(["--expect-plan-sha256", &digest])
        .output()
        .unwrap();
    let values = frames(&output, 0);
    let result = values.last().unwrap();
    assert_eq!(result["status"], "succeeded");
    assert_eq!(result["output_state"], "published");
    assert_eq!(result["cleanup_state"], "complete");
    assert_eq!(result["plan"]["private_key_access_checked"], true);
    let actual: String = real_fixture::hash(&fixture.output)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    assert_eq!(result["artifact"]["sha256"], actual);
    let zipalign = std::env::var_os("SHIELD_AGENT_TEST_ZIPALIGN").expect("请指定 SDK zipalign");
    assert!(shield_core::utils::no_window_command(zipalign)
        .args(["-c", "-P", "16", "4"])
        .arg(&fixture.output)
        .output()
        .unwrap()
        .status
        .success());
    let input_cert =
        shield_core::extract_apk_cert_fingerprint(&fixture.input, Some(&fixture.signer)).unwrap();
    let output_cert =
        shield_core::extract_apk_cert_fingerprint(&fixture.output, Some(&fixture.signer)).unwrap();
    assert_eq!(input_cert, output_cert);
    fixture.assert_inputs_unchanged();
    fixture.assert_no_tasks();

    let fixture = real_fixture::Fixture::new();
    let old_digest = real_plan(&fixture);
    fs::OpenOptions::new()
        .append(true)
        .open(&fixture.config)
        .unwrap()
        .write_all("\n# 用户在规划后改了临时配置\n".as_bytes())
        .unwrap();
    let output = real_command(&fixture, "protect")
        .args(["--expect-plan-sha256", &old_digest])
        .output()
        .unwrap();
    let values = frames(&output, 1);
    assert_eq!(values.last().unwrap()["code"], "PLAN_MISMATCH");
    assert!(!fixture.output.exists());
    fixture.assert_inputs_unchanged();
    fixture.assert_no_tasks();
}

#[test]
#[ignore = "需要本机 JDK、资源和授权小型 APK，显式验收执行阶段取消，不纳入日常 CI"]
fn 真实自动化执行阶段信号取消后无最终产物() {
    use std::io::BufRead;
    let fixture = real_fixture::Fixture::new();
    let digest = real_plan(&fixture);
    let mut command = real_command(&fixture, "protect");
    command.args(["--expect-plan-sha256", &digest]);
    let mut running = Running::spawn(&mut command);
    let stdout = running.child().stdout.take().unwrap();
    let (sender, receiver) = std::sync::mpsc::channel();
    let reader = std::thread::spawn(move || {
        let mut collected = Vec::new();
        for line in std::io::BufReader::new(stdout).lines() {
            let line = line.unwrap();
            let value: Value = serde_json::from_str(&line).unwrap();
            if value["stage"] == "unpack" {
                let _ = sender.send(());
            }
            collected.extend_from_slice(line.as_bytes());
            collected.push(b'\n');
        }
        collected
    });
    assert!(
        receiver.recv_timeout(Duration::from_secs(40)).is_ok(),
        "必须进入执行解包阶段后再取消"
    );
    assert!(running.child().try_wait().unwrap().is_none());
    assert_eq!(
        unsafe { libc::kill(running.child().id() as i32, libc::SIGINT) },
        0
    );
    let mut output = running.finish();
    output.stdout = reader.join().unwrap();
    let values = frames(&output, 130);
    assert_eq!(values.last().unwrap()["status"], "cancelled");
    assert_eq!(values.last().unwrap()["output_state"], "not_published");
    assert!(!fixture.output.exists());
    fixture.assert_inputs_unchanged();
    fixture.assert_no_tasks();
}

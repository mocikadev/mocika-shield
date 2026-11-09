//! 调用显式构建的真实工作入口，仅在临时 Java 启动器中注入故障，不扩张正式请求。
use super::*;
use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::thread;

#[path = "../../../tests/support/execution_fixture.rs"]
mod fixture;
use fixture::{private, Fixture};

fn program() -> PathBuf {
    let path = PathBuf::from(
        std::env::var_os("SHIELD_AGENT_TEST_EXECUTABLE")
            .expect("请先显式构建开发特性的 CLI，再指定其绝对路径"),
    );
    assert!(path.is_absolute() && path.is_file(), "测试执行器路径无效");
    path
}

fn request(fixture: &Fixture, timeout_seconds: u64) -> Request {
    Request {
        config: Some(fixture.config.clone()),
        shared: None,
        input: fixture.input.clone(),
        output: fixture.output.clone(),
        runtime_profile: Profile::Standard,
        exclude_abis: vec![],
        timeout_seconds,
    }
}

#[test]
#[ignore = "需要显式开发 CLI、真实 JDK 和自有 APK，验证共享提交检查，不纳入日常 CI"]
fn 共享材料在最终提交前变化必须拒绝交付() {
    use shield_app::{
        certificates::{
            CertificatePreferencesPatch, CertificateStoreState, CertificateUpsertInput,
        },
        paths::ApplicationPaths,
    };
    let fixture = Fixture::new();
    let program = program();
    let home = fixture.root().join("共享提交校验");
    let paths = ApplicationPaths::isolated(&home).unwrap();
    let store =
        CertificateStoreState::initialize(paths.certificate_database(), paths.keystore_dir())
            .unwrap();
    let input = CertificateUpsertInput {
        id: None,
        name: "提交检查夹具".into(),
        source_type: "external".into(),
        keystore_path: String::new(),
        keystore_password: Fixture::password().into(),
        key_alias: "agentfixture".into(),
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
    let record = store
        .save_certificate(
            &input,
            fixture.root().join("临时测试证书.p12").to_str().unwrap(),
            "agentfixture",
            None,
        )
        .unwrap();
    let mut request = request(&fixture, 120);
    request.config = None;
    request.shared = Some(crate::automation::shared_plan::SharedRequest {
        app_home: Some(home),
        certificate_id: None,
        use_default_certificate: true,
        runtime_profile: None,
        environment_policy: None,
        binding: None,
    });
    let mut reached = false;
    let value = result(run_with_options(
        request,
        Arc::new(AtomicBool::new(false)),
        || {
            let mut command = fixture.command(&program);
            command.env_remove("MOCIKA_SHIELD_KS_PASS");
            command
        },
        RunMode::Probe,
        |notice| {
            if matches!(notice, Notice::Publishing) {
                reached = true;
                store
                    .patch_certificate_preferences(
                        &record.id,
                        CertificatePreferencesPatch {
                            sign_v3: Some(false),
                            ..Default::default()
                        },
                        false,
                    )
                    .unwrap();
            }
        },
    ));
    assert!(reached, "须到达唯一提交者的最终提交门槛");
    assert_finished(&fixture, &value, "failed");
    assert_eq!(value["code"], "PLAN_SOURCE_CHANGED");
}

fn result(result: Result<Value, CheckError>) -> Value {
    result.unwrap_or_else(|error| panic!("执行器未返回终态：{}", error.code))
}

fn assert_finished(fixture: &Fixture, value: &Value, status: &str) {
    assert_eq!(value["status"], status, "有限结果：{value}");
    assert_eq!(value["output_state"], "not_published");
    assert_eq!(value["cleanup_complete"], true);
    assert_eq!(value["retryable"], false);
    assert!(value.get("artifact").is_none());
    assert!(!value.to_string().contains(Fixture::password()));
    assert!(!value.to_string().contains(fixture.root().to_str().unwrap()));
    assert!(fixture.signer.is_file());
    fixture.assert_inputs_unchanged();
    fixture.assert_no_tasks();
}

struct Gate {
    bin: PathBuf,
    root: PathBuf,
    java: PathBuf,
}

impl Gate {
    fn new(fixture: &Fixture) -> Self {
        let root = fixture.root().join("故障注入记录");
        let bin = root.join("bin");
        fs::create_dir_all(&bin).unwrap();
        let java = shield_core::utils::find_java().unwrap();
        let keytool = shield_core::utils::find_keytool().unwrap();
        private(&bin.join("java"), r#"#!/bin/sh
case " $* " in
  *'/materials/apktool.jar d '*)
    echo "$$" > "$SHIELD_FIXTURE_GATE/java-pid"
    if [ "$SHIELD_FIXTURE_MODE" = fail ]; then
        echo "故障注入原文：$MOCIKA_SHIELD_KS_PASS $SHIELD_FIXTURE_GATE" >&2
        exit 42
    fi
    /bin/sh -c 'echo "$$" > "$1/child-pid"; while :; do printf x >> "$1/heartbeat"; /bin/sleep 0.02; done' fixture "$SHIELD_FIXTURE_GATE" &
    child=$!
    while [ ! -e "$SHIELD_FIXTURE_GATE/release" ]; do /bin/sleep 0.02; done
    /bin/kill "$child"
    wait "$child" 2>/dev/null
    ;;
esac
exec "$SHIELD_FIXTURE_REAL_JAVA" "$@"
"#.as_bytes());
        fs::set_permissions(bin.join("java"), fs::Permissions::from_mode(0o700)).unwrap();
        std::os::unix::fs::symlink(keytool, bin.join("keytool")).unwrap();
        Self { bin, root, java }
    }

    fn command(&self, fixture: &Fixture, program: &Path, mode: &str) -> Command {
        let mut command = fixture.command(program);
        command
            .env("PATH", &self.bin)
            .env_remove("JAVA_HOME")
            .env("SHIELD_FIXTURE_GATE", &self.root)
            .env("SHIELD_FIXTURE_MODE", mode)
            .env("SHIELD_FIXTURE_REAL_JAVA", &self.java);
        command
    }

    fn ready(&self) -> bool {
        let deadline = Instant::now() + Duration::from_secs(30);
        while Instant::now() < deadline {
            if self.root.join("java-pid").is_file()
                && self.root.join("child-pid").is_file()
                && self
                    .root
                    .join("heartbeat")
                    .metadata()
                    .is_ok_and(|m| m.len() > 0)
            {
                return true;
            }
            thread::sleep(Duration::from_millis(10));
        }
        false
    }

    fn release(&self) {
        private(&self.root.join("release"), b"continue");
    }

    fn assert_stopped(&self) {
        let before = fs::read(self.root.join("heartbeat")).unwrap();
        thread::sleep(Duration::from_millis(100));
        assert_eq!(
            fs::read(self.root.join("heartbeat")).unwrap(),
            before,
            "终态之后后代仍在写入"
        );
    }
}

fn at_gate(fixture: &Fixture, timeout: u64, action: impl FnOnce(&Gate, &Arc<AtomicBool>)) -> Value {
    let program = program();
    at_gate_using(fixture, &program, timeout, action)
}

fn at_gate_using(
    fixture: &Fixture,
    program: &Path,
    timeout: u64,
    action: impl FnOnce(&Gate, &Arc<AtomicBool>),
) -> Value {
    let gate = Gate::new(fixture);
    let cancel = Arc::new(AtomicBool::new(false));
    let value = thread::scope(|scope| {
        let handle = scope.spawn(|| {
            run_with_command(request(fixture, timeout), cancel.clone(), || {
                gate.command(fixture, program, "wait")
            })
        });
        let ready = gate.ready();
        if ready {
            action(&gate, &cancel);
        } else {
            cancel.store(true, Ordering::SeqCst);
        }
        let value = result(handle.join().unwrap());
        assert!(ready, "真实执行没有到达解包门槛，有限结果：{value}");
        value
    });
    gate.assert_stopped();
    value
}

#[test]
#[ignore = "需要显式构建的开发 CLI、真实 JDK、资源及授权的小型测试 APK，不纳入日常 CI"]
fn 真实工作入口不允许两阶段切换执行器文件() {
    let fixture = Fixture::new();
    let program = program();
    let other = fixture.root().join("相同字节的另一个执行器");
    fs::copy(&program, &other).unwrap();
    let mut launches = 0;
    let outcome = run_with_command(
        request(&fixture, 60),
        Arc::new(AtomicBool::new(false)),
        || {
            launches += 1;
            fixture.command(if launches == 1 { &program } else { &other })
        },
    );
    assert!(matches!(
        outcome,
        Err(CheckError {
            code: "EXECUTOR_CHANGED",
            ..
        })
    ));
    assert_eq!(launches, 2);
    assert!(!fixture.output.exists());
    fixture.assert_inputs_unchanged();
    fixture.assert_no_tasks();
}

#[test]
#[ignore = "需要显式构建的开发 CLI、真实 JDK、资源及授权的小型测试 APK，不纳入日常 CI"]
fn 真实工作入口执行器被同字节替换仍拒绝交付() {
    let fixture = Fixture::new();
    let source = program();
    let executor = fixture.root().join("私有测试执行器");
    fs::copy(&source, &executor).unwrap();
    let value = at_gate_using(&fixture, &executor, 60, |gate, _| {
        let replacement = fixture.root().join("待替换的执行器");
        fs::copy(&source, &replacement).unwrap();
        fs::rename(replacement, &executor).unwrap();
        gate.release();
    });
    assert_finished(&fixture, &value, "failed");
    assert_eq!(value["code"], "EXECUTOR_CHANGED");
    assert!(!fixture.output.exists());
}

#[test]
#[ignore = "需要显式构建的开发 CLI、真实 JDK、资源及授权的小型测试 APK，不纳入日常 CI"]
fn 真实工作入口执行中运行时被同字节替换拒绝交付() {
    let fixture = Fixture::new();
    let value = at_gate(&fixture, 60, |gate, _| {
        let replacement = gate.bin.join("java-replacement");
        fs::copy(gate.bin.join("java"), &replacement).unwrap();
        fs::rename(replacement, gate.bin.join("java")).unwrap();
        gate.release();
    });
    assert_finished(&fixture, &value, "failed");
    assert_eq!(value["code"], "TOOLCHAIN_CHANGED");
    assert!(!fixture.output.exists());
}

#[test]
#[ignore = "需要显式构建的开发 CLI、真实 JDK、资源及授权的小型测试 APK，不纳入日常 CI"]
fn 真实工作入口执行中证书工具链接改向拒绝交付() {
    let fixture = Fixture::new();
    let value = at_gate(&fixture, 60, |gate, _| {
        let replacement = gate.bin.join("不同的证书工具");
        private(&replacement, b"#!/bin/sh\nexit 42\n");
        fs::set_permissions(&replacement, fs::Permissions::from_mode(0o700)).unwrap();
        let link = gate.bin.join("keytool");
        fs::remove_file(&link).unwrap();
        std::os::unix::fs::symlink(replacement, link).unwrap();
        gate.release();
    });
    assert_finished(&fixture, &value, "failed");
    assert_eq!(value["code"], "TOOLCHAIN_CHANGED");
    assert!(!fixture.output.exists());
}

#[test]
#[ignore = "需要显式构建的开发 CLI、真实 JDK、资源及授权的小型测试 APK，不纳入日常 CI"]
fn 真实工作入口保留计划运行时而不静默切换路径中的同名工具() {
    let fixture = Fixture::new();
    let bin = fixture.root().join("另一个工具目录");
    fs::create_dir(&bin).unwrap();
    private(
        &bin.join("java"),
        b"#!/bin/sh\necho unexpected > \"$SHIELD_FIXTURE_UNEXPECTED\"\nexit 42\n",
    );
    fs::set_permissions(bin.join("java"), fs::Permissions::from_mode(0o700)).unwrap();
    let marker = fixture.root().join("不应执行的工具标记");
    let program = program();
    let mut launches = 0;
    let value = result(run_with_command(
        request(&fixture, 60),
        Arc::new(AtomicBool::new(false)),
        || {
            launches += 1;
            let mut command = fixture.command(&program);
            if launches == 2 {
                command
                    .env("PATH", &bin)
                    .env("SHIELD_FIXTURE_UNEXPECTED", &marker);
            }
            command
        },
    ));
    assert_eq!(value["status"], "succeeded", "有限结果：{value}");
    assert_eq!(value["output_state"], "published");
    assert_eq!(value["cleanup_complete"], true);
    assert!(!marker.exists());
    assert_eq!(
        value["artifact"]["sha256"],
        hex(&fixture::hash(&fixture.output))
    );
    let input =
        shield_core::extract_apk_cert_fingerprint(&fixture.input, Some(&fixture.signer)).unwrap();
    let output =
        shield_core::extract_apk_cert_fingerprint(&fixture.output, Some(&fixture.signer)).unwrap();
    assert_eq!(input, output);
    fixture.assert_inputs_unchanged();
    fixture.assert_no_tasks();
}

#[test]
fn 预先取消不创建任务或启动工作进程() {
    let root = tempfile::tempdir().unwrap();
    let request = Request {
        config: Some(root.path().join("none.toml")),
        shared: None,
        input: root.path().join("none.apk"),
        output: root.path().join("output.apk"),
        runtime_profile: Profile::Standard,
        exclude_abis: vec![],
        timeout_seconds: 30,
    };
    let outcome = run_with_command(request, Arc::new(AtomicBool::new(true)), || {
        panic!("取消后不得启动工作进程")
    });
    assert!(matches!(
        outcome,
        Err(CheckError {
            code: "TASK_CANCELLED",
            ..
        })
    ));
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 0);
}

#[test]
#[ignore = "需要显式构建的开发 CLI、真实 JDK、资源及授权的小型测试 APK，不纳入日常 CI"]
fn 真实工作入口执行失败不发布且不透传工具原文() {
    let fixture = Fixture::new();
    let gate = Gate::new(&fixture);
    let program = program();
    let value = result(run_with_command(
        request(&fixture, 60),
        Arc::new(AtomicBool::new(false)),
        || gate.command(&fixture, &program, "fail"),
    ));
    assert_finished(&fixture, &value, "failed");
    assert_eq!(value["code"], "TOOL_PROCESS_FAILED");
    assert!(!value.to_string().contains("故障注入原文"));
    assert!(!fixture.output.exists());
}

#[test]
#[ignore = "需要显式构建的开发 CLI、真实 JDK、资源及授权的小型测试 APK，不纳入日常 CI"]
fn 真实工作入口内部取消后后代停止且无最终文件() {
    let fixture = Fixture::new();
    let value = at_gate(&fixture, 60, |_, cancel| {
        cancel.store(true, Ordering::SeqCst)
    });
    assert_finished(&fixture, &value, "cancelled");
    assert!(!fixture.output.exists());
}

#[test]
#[ignore = "需要显式构建的开发 CLI、真实 JDK、资源及授权的小型测试 APK，不纳入日常 CI"]
fn 真实工作入口执行阶段超时后后代停止且无最终文件() {
    let fixture = Fixture::new();
    let started = Instant::now();
    let value = at_gate(&fixture, 15, |_, _| {});
    assert_finished(&fixture, &value, "timed_out");
    assert!(started.elapsed() < Duration::from_secs(22));
    assert!(!fixture.output.exists());
}

#[test]
#[ignore = "需要显式构建的开发 CLI、真实 JDK、资源及授权的小型测试 APK，不纳入日常 CI"]
fn 真实工作入口准备后计划变化拒绝旧摘要() {
    let fixture = Fixture::new();
    let program = program();
    let mut launches = 0;
    let value = result(run_with_command(
        request(&fixture, 60),
        Arc::new(AtomicBool::new(false)),
        || {
            launches += 1;
            if launches == 2 {
                fs::OpenOptions::new()
                    .append(true)
                    .open(&fixture.config)
                    .unwrap()
                    .write_all("\n# 本次临时配置在执行前发生变化\n".as_bytes())
                    .unwrap();
            }
            fixture.command(&program)
        },
    ));
    assert_eq!(launches, 2);
    assert_finished(&fixture, &value, "failed");
    assert_eq!(value["code"], "PLAN_MISMATCH");
    assert!(!fixture.output.exists());
}

#[test]
#[ignore = "需要显式构建的开发 CLI、真实 JDK、资源及授权的小型测试 APK，不纳入日常 CI"]
fn 真实工作入口执行中源材料变化拒绝交付() {
    let fixture = Fixture::new();
    let value = at_gate(&fixture, 60, |gate, _| {
        fs::OpenOptions::new()
            .append(true)
            .open(&fixture.config)
            .unwrap()
            .write_all("\n# 本次临时配置在执行中发生变化\n".as_bytes())
            .unwrap();
        gate.release();
    });
    assert_finished(&fixture, &value, "failed");
    assert_eq!(value["code"], "PLAN_SOURCE_CHANGED");
    assert!(!fixture.output.exists());
}

#[test]
#[ignore = "需要显式构建的开发 CLI、真实 JDK、资源及授权的小型测试 APK，不纳入日常 CI"]
fn 真实工作入口执行中输出被占用不会覆盖() {
    let fixture = Fixture::new();
    let value = at_gate(&fixture, 60, |gate, _| {
        private(&fixture.output, "其他任务已交付的内容".as_bytes());
        gate.release();
    });
    assert_finished(&fixture, &value, "failed");
    assert_eq!(
        fs::read(&fixture.output).unwrap(),
        "其他任务已交付的内容".as_bytes()
    );
}

#[test]
#[ignore = "需要显式构建的开发 CLI、真实 JDK、资源及授权的小型测试 APK，不纳入日常 CI"]
fn 真实工作入口工作进程异常退出仍停止其后代() {
    let fixture = Fixture::new();
    let value = at_gate(&fixture, 60, |gate, _| {
        let pid: i32 = fs::read_to_string(gate.root.join("java-pid"))
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        let group = rustix::process::getpgid(rustix::process::Pid::from_raw(pid)).unwrap();
        assert_ne!(
            group,
            rustix::process::getpgrp(),
            "只能终止测试创建的独立工作进程"
        );
        rustix::process::kill_process(group, rustix::process::Signal::KILL).unwrap();
    });
    assert_finished(&fixture, &value, "failed");
    assert!(!fixture.output.exists());
}

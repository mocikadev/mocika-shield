//! 准备阶段的真实子进程控制；工具为故障夹具，不代表真实 APK 签名验证。
use super::*;
use std::{
    io::Write,
    os::unix::fs::PermissionsExt,
    process::Child,
    time::{Duration, Instant},
};

struct Gate {
    root: tempfile::TempDir,
    child: Option<Child>,
    real_java: Option<PathBuf>,
    completion_deadline: Option<Instant>,
}
impl Gate {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let _store = record(root.path(), [true, true, true, false]);
        let bin = root.path().join("bin");
        fs::create_dir(&bin).unwrap();
        fs::write(
            bin.join("java"),
            "#!/bin/sh\necho 'openjdk version \"17.0.1\"' >&2\n",
        )
        .unwrap();
        fs::write(bin.join("keytool"), "#!/bin/sh\nwhile :; do printf x >> \"$SHIELD_SIGN_GATE/heartbeat\"; /bin/sleep 0.02; done\n").unwrap();
        for name in ["java", "keytool"] {
            fs::set_permissions(bin.join(name), fs::Permissions::from_mode(0o700)).unwrap();
        }
        let file = fs::File::create(root.path().join("input.apk")).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        zip.start_file(
            "AndroidManifest.xml",
            zip::write::SimpleFileOptions::default(),
        )
        .unwrap();
        zip.write_all(b"fixture").unwrap();
        zip.finish().unwrap();
        fs::write(root.path().join("tool.jar"), b"fixture").unwrap();
        fs::write(
            root.path().join("shield-cli.toml"),
            "schema_version=1\n[sign]\napksigner='tool.jar'\n",
        )
        .unwrap();
        Self {
            root,
            child: None,
            real_java: None,
            completion_deadline: None,
        }
    }
    fn start(&mut self, timeout: u64, control: bool) {
        self.spawn(timeout, control);
        self.wait_ready();
    }
    fn spawn(&mut self, timeout: u64, control: bool) {
        // 从启动到确认停止之前都保留夹具；断言失败不能抢删可能仍活动的材料。
        self.root.disable_cleanup(true);
        let p = self.root.path();
        let mut cmd = command_timeout(
            p,
            Some(&p.join("shield-cli.toml")),
            "sign",
            &p.join("input.apk"),
            &p.join("output.apk"),
            timeout,
        );
        if control {
            cmd.arg("--control-stdin");
        }
        if let Some(java) = &self.real_java {
            cmd.env("SHIELD_SIGN_REAL_JAVA", java);
        }
        self.completion_deadline = Some(Instant::now() + Duration::from_secs(timeout + 10));
        self.child = Some(
            cmd.env("PATH", p.join("bin"))
                .env_remove("JAVA_HOME")
                .env("SHIELD_SIGN_GATE", p)
                .stdin(Stdio::piped())
                // 用本夹具文件持续接收，不能在等待进程退出时把事件管道留到满缓冲。
                .stdout(fs::File::create(p.join("events.jsonl")).unwrap())
                .stderr(fs::File::create(p.join("errors.txt")).unwrap())
                .spawn()
                .unwrap(),
        );
    }
    fn wait_ready(&mut self) {
        let p = self.root.path();
        let until = Instant::now() + Duration::from_secs(10);
        while !(p.join("heartbeat").metadata().is_ok_and(|m| m.len() > 0)
            && (self.real_java.is_none()
                || p.join("child-heartbeat")
                    .metadata()
                    .is_ok_and(|m| m.len() > 0)))
        {
            if self.child.as_mut().unwrap().try_wait().unwrap().is_some() {
                let output = self.output();
                let value = result(output, false);
                panic!(
                    "到达故障门槛前已退出：状态={}，分类={}，耗时={}毫秒",
                    value["status"], value["code"], value["elapsed_ms"]
                );
            }
            assert!(Instant::now() < until, "准备工作进程未到达故障门槛");
            std::thread::sleep(Duration::from_millis(10));
        }
    }
    fn finish(&mut self, exit: i32, code: &str) {
        self.finish_checked(exit, code, true);
    }
    fn output(&mut self) -> Output {
        let status = self.child.take().unwrap().wait().unwrap();
        let capture = |name| {
            let path = self.root.path().join(name);
            assert!(path.metadata().unwrap().len() <= 128 * 1024, "测试事件超限");
            fs::read(path).unwrap()
        };
        Output {
            status,
            stdout: capture("events.jsonl"),
            stderr: capture("errors.txt"),
        }
    }
    fn finish_checked(&mut self, exit: i32, code: &str, require_tool: bool) -> bool {
        while self.child.as_mut().unwrap().try_wait().unwrap().is_none() {
            assert!(
                Instant::now() < self.completion_deadline.unwrap(),
                "任务在总期限及收尾宽限之后仍未退出"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        let output = self.output();
        assert_eq!(output.status.code(), Some(exit));
        let value = result(output, false);
        assert_eq!(value["code"], code);
        assert_eq!(value["output_state"], "not_published");
        assert_eq!(value["cleanup_state"], "complete");
        assert!(!self.root.path().join("output.apk").exists());
        let heartbeat = self.root.path().join("heartbeat");
        let tool_started = heartbeat.try_exists().unwrap();
        assert!(
            !require_tool || tool_started,
            "本测试必须确认工具实际运行过"
        );
        if tool_started {
            let before = fs::read(&heartbeat).unwrap();
            std::thread::sleep(Duration::from_millis(100));
            assert_eq!(fs::read(heartbeat).unwrap(), before);
        }
        assert!(fs::read_dir(self.root.path()).unwrap().all(|e| !e
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".mocika-")));
        self.root.disable_cleanup(false);
        tool_started
    }
}
impl Drop for Gate {
    fn drop(&mut self) {
        if let Some(child) = &mut self.child {
            // 失败时先终止父端；工作租约负责停止工作组，不在其活动时递归清理。
            let _ = child.kill();
            let _ = child.wait();
            // tempfile 的回收在正常有限收尾之后；断言失败时保守保留目录。
            self.root.disable_cleanup(true);
        }
    }
}
#[test]
fn 准备期间信号取消停止真实工具且不发布() {
    let mut gate = Gate::new();
    gate.start(30, false);
    let pid = gate.child.as_ref().unwrap().id();
    assert_eq!(unsafe { libc::kill(pid as i32, libc::SIGINT) }, 0);
    gate.finish(130, "TASK_CANCELLED");
}
#[test]
fn 准备期间超时停止真实工具且不发布() {
    let mut gate = Gate::new();
    // 本例必须先确认工具挂起；启动检查有独立余量，不再假定调试构建三秒内到达工具。
    // 任务仍使用从启动开始的固定总期限，不在就绪后重置；原三秒边界由下例单独验证。
    gate.start(15, false);
    gate.finish(124, "TASK_TIMEOUT");
}
#[test]
fn 准备总期限不假定工具已启动() {
    let mut gate = Gate::new();
    gate.spawn(3, false);
    let tool_started = gate.finish_checked(124, "TASK_TIMEOUT", false);
    eprintln!("三秒总期限：已确认超时且无交付、清理完成；工具实际启动={tool_started}");
}
#[test]
fn 准备期间标准输入取消和断开都不发布() {
    for closed in [false, true] {
        let mut gate = Gate::new();
        gate.start(30, true);
        let mut stdin = gate.child.as_mut().unwrap().stdin.take().unwrap();
        if !closed {
            stdin.write_all(b"{\"type\":\"cancel\"}\n").unwrap();
            stdin.flush().unwrap();
        }
        if closed {
            drop(stdin);
        } else { /* 保留有效取消后的输入，避免测试混入 EOF 原因。 */
        }
        gate.finish(
            130,
            if closed {
                "CLIENT_DISCONNECTED"
            } else {
                "TASK_CANCELLED"
            },
        );
    }
}

#[test]
#[ignore = "需要真实 JDK、自有 SHIELD_TEST_APK 和 SHIELD_TEST_APKSIGNER，挂起真正签名步骤"]
fn 真实签名步骤取消超时和断开均停止工作组而不交付() {
    for mode in ["cancel", "timeout", "closed", "tool-change"] {
        let mut gate = Gate::new();
        let root = gate.root.path();
        let paths = ApplicationPaths::isolated(root).unwrap();
        let store = CertificateStoreState::open_existing(
            paths.certificate_database(),
            paths.keystore_dir(),
        )
        .unwrap();
        create_managed_certificate(
            &store,
            CreateManagedCertificateInput {
                name: "真实控制夹具".into(),
                file_name: "真实控制夹具".into(),
                key_alias: "release".into(),
                keystore_password: SECRET.into(),
                key_password: String::new(),
                ks_type: Some("PKCS12".into()),
                sign_v1: true,
                sign_v2: true,
                sign_v3: true,
                sign_v4: false,
                auto_sign_enabled: false,
                note: String::new(),
                set_as_default: true,
                dname: "CN=真实控制夹具".into(),
                validity_days: 7,
                key_size: 2048,
            },
        )
        .unwrap();
        let input = PathBuf::from(std::env::var_os("SHIELD_TEST_APK").expect("指定自有 APK"));
        let signer =
            PathBuf::from(std::env::var_os("SHIELD_TEST_APKSIGNER").expect("指定签名工具"));
        fs::copy(input, root.join("input.apk")).unwrap();
        let before = fs::read(root.join("input.apk")).unwrap();
        fs::write(
            root.join("shield-cli.toml"),
            format!(
                "schema_version=1\n[sign]\napksigner={}\n",
                serde_json::to_string(signer.to_str().unwrap()).unwrap()
            ),
        )
        .unwrap();
        let bin = root.join("bin");
        fs::remove_file(bin.join("keytool")).unwrap();
        std::os::unix::fs::symlink(
            shield_core::utils::find_keytool().unwrap(),
            bin.join("keytool"),
        )
        .unwrap();
        fs::write(bin.join("java"), r#"#!/bin/sh
case " $* " in
  *'/materials/apksigner.jar sign '*)
    /bin/sh -c 'while :; do printf x >> "$SHIELD_SIGN_GATE/child-heartbeat"; /bin/sleep 0.02; done' &
    fixture_child=$!
    while [ ! -e "$SHIELD_SIGN_GATE/release" ]; do printf x >> "$SHIELD_SIGN_GATE/heartbeat"; /bin/sleep 0.02; done
    /bin/kill "$fixture_child"
    wait "$fixture_child" 2>/dev/null
    ;;
esac
exec "$SHIELD_SIGN_REAL_JAVA" "$@"
"#).unwrap();
        gate.real_java = Some(shield_core::utils::find_java().unwrap());
        gate.start(20, mode == "closed");
        match mode {
            "cancel" => assert_eq!(
                unsafe { libc::kill(gate.child.as_ref().unwrap().id() as i32, libc::SIGINT) },
                0
            ),
            "closed" => drop(gate.child.as_mut().unwrap().stdin.take()),
            "tool-change" => {
                let mut file = fs::OpenOptions::new()
                    .append(true)
                    .open(gate.root.path().join("bin/java"))
                    .unwrap();
                file.write_all("\n# 测试工具变化\n".as_bytes()).unwrap();
                file.sync_all().unwrap();
                fs::write(gate.root.path().join("release"), b"continue").unwrap();
            }
            _ => {}
        }
        gate.finish(
            match mode {
                "timeout" => 124,
                "tool-change" => 1,
                _ => 130,
            },
            match mode {
                "timeout" => "TASK_TIMEOUT",
                "closed" => "CLIENT_DISCONNECTED",
                "tool-change" => "TOOLCHAIN_CHANGED",
                _ => "TASK_CANCELLED",
            },
        );
        let root = gate.root.path();
        assert_eq!(fs::read(root.join("input.apk")).unwrap(), before);
        let child_before = fs::read(root.join("child-heartbeat")).unwrap();
        std::thread::sleep(Duration::from_millis(100));
        assert_eq!(
            fs::read(root.join("child-heartbeat")).unwrap(),
            child_before
        );
    }
}

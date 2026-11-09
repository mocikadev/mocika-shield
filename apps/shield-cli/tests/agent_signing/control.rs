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
        }
    }
    fn start(&mut self, timeout: u64, control: bool) {
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
        self.child = Some(
            cmd.env("PATH", p.join("bin"))
                .env_remove("JAVA_HOME")
                .env("SHIELD_SIGN_GATE", p)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap(),
        );
        let until = Instant::now() + Duration::from_secs(10);
        while !(p.join("heartbeat").metadata().is_ok_and(|m| m.len() > 0)
            && (self.real_java.is_none()
                || p.join("child-heartbeat")
                    .metadata()
                    .is_ok_and(|m| m.len() > 0)))
        {
            assert!(Instant::now() < until, "准备工作进程未到达故障门槛");
            std::thread::sleep(Duration::from_millis(10));
        }
    }
    fn finish(&mut self, exit: i32, code: &str) {
        let output = self.child.take().unwrap().wait_with_output().unwrap();
        assert_eq!(output.status.code(), Some(exit));
        let value = result(output, false);
        assert_eq!(value["code"], code);
        assert_eq!(value["output_state"], "not_published");
        assert!(!self.root.path().join("output.apk").exists());
        let before = fs::read(self.root.path().join("heartbeat")).unwrap();
        std::thread::sleep(Duration::from_millis(100));
        assert_eq!(
            fs::read(self.root.path().join("heartbeat")).unwrap(),
            before
        );
        assert!(fs::read_dir(self.root.path()).unwrap().all(|e| !e
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".mocika-")));
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
    gate.start(3, false);
    gate.finish(124, "TASK_TIMEOUT");
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

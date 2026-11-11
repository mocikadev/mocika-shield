//! 跨平台正式入口的真实签名与故障清场；只操作自有 APK 和隔离资料。
#![cfg(all(
    feature = "mcp-server",
    any(target_os = "macos", target_os = "linux", windows)
))]
use serde_json::{json, Value};
#[cfg(not(windows))]
use shield_services::certificates::create_managed_certificate;
use shield_services::{
    certificates::{CertificateStoreState, CreateManagedCertificateInput},
    paths::ApplicationPaths,
};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};
#[path = "agent_desktop/certificates.rs"]
mod certificates;
#[path = "support/mcp_client.rs"]
mod client;
#[path = "agent_desktop/ordinary.rs"]
mod ordinary;

const SECRET: &str = "native-desktop-fixture-password";
struct Fixture {
    root: tempfile::TempDir,
    java: PathBuf,
    keytool: PathBuf,
    certificate: String,
    source: Vec<u8>,
}
impl Fixture {
    fn new() -> Self {
        let apk = PathBuf::from(std::env::var_os("SHIELD_TEST_APK").expect("指定自有 APK"));
        let signer =
            PathBuf::from(std::env::var_os("SHIELD_TEST_APKSIGNER").expect("指定真实签名工具"));
        let root = tempfile::Builder::new()
            .prefix("原生入口 中文空格")
            .tempdir()
            .unwrap();
        let p = root.path();
        for dir in ["input", "output", "jdk/bin"] {
            fs::create_dir_all(p.join(dir)).unwrap();
        }
        let source = fs::read(&apk).unwrap();
        fs::write(p.join("input/app.apk"), &source).unwrap();
        fs::write(
            p.join("shield-cli.toml"),
            format!(
                "schema_version=1\n[sign]\napksigner={}\n",
                serde_json::to_string(signer.to_str().unwrap()).unwrap()
            ),
        )
        .unwrap();
        if let Some(resources) = std::env::var_os("SHIELD_TEST_STANDARD_RESOURCES") {
            let apktool = Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../tools/apktool_3.0.1.jar")
                .canonicalize()
                .unwrap();
            use std::io::Write;
            let mut config = fs::OpenOptions::new()
                .append(true)
                .open(p.join("shield-cli.toml"))
                .unwrap();
            write!(
                config,
                "\n[protect]\napktool={}\nresources={}\n",
                serde_json::to_string(apktool.to_str().unwrap()).unwrap(),
                serde_json::to_string(Path::new(&resources).to_str().unwrap()).unwrap()
            )
            .unwrap();
        }
        let paths = ApplicationPaths::isolated(&p.join("home")).unwrap();
        let store =
            CertificateStoreState::initialize(paths.certificate_database(), paths.keystore_dir())
                .unwrap();
        let input = CreateManagedCertificateInput {
            name: "原生测试证书".into(),
            file_name: "原生测试证书".into(),
            key_alias: "中文Release".into(),
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
            dname: "CN=原生入口测试".into(),
            validity_days: 7,
            key_size: 2048,
        };
        #[cfg(not(windows))]
        let certificate = create_managed_certificate(&store, input);
        #[cfg(windows)]
        let certificate = {
            let workspace = p.join("certificate-work");
            shield_core::windows_private::create_directory(&workspace).unwrap();
            let adapter = Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../tools/java-adapter/build/java-adapter.jar")
                .canonicalize()
                .unwrap();
            let context = shield_core::JavaToolContext::new(&adapter, &workspace).unwrap();
            shield_services::certificates::create_managed_certificate_with_context(
                &store,
                input,
                &context,
                &shield_core::utils::find_java().unwrap(),
                &shield_core::utils::find_keytool().unwrap(),
            )
        };
        let certificate = certificate.unwrap().id;
        Self {
            root,
            java: shield_core::utils::find_java().unwrap(),
            keytool: shield_core::utils::find_keytool().unwrap(),
            certificate,
            source,
        }
    }
    fn native_tool(&self) {
        let target = self
            .root
            .path()
            .join("jdk/bin")
            .join(format!("java{}", std::env::consts::EXE_SUFFIX));
        let source = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/agent-signing/NativeTool.rs");
        assert!(shield_core::utils::no_window_command("rustc")
            .arg("--edition=2021")
            .arg("--crate-name=native_tool_fixture")
            .arg(source)
            .arg("-o")
            .arg(&target)
            .status()
            .unwrap()
            .success());
        fs::copy(
            target,
            self.root
                .path()
                .join("jdk/bin")
                .join(format!("keytool{}", std::env::consts::EXE_SUFFIX)),
        )
        .unwrap();
    }
    fn command(&self, hanging: bool) -> Command {
        let p = self.root.path();
        let mut cmd = shield_core::utils::no_window_command(env!("CARGO_BIN_EXE_shield"));
        cmd.current_dir(p)
            .arg("--app-home")
            .arg(p.join("home"))
            .arg("--config")
            .arg(p.join("shield-cli.toml"))
            .env_remove("MOCIKA_SHIELD_KS_PASS")
            .env_remove("MOCIKA_SHIELD_KEY_PASS")
            .env_remove("JAVA_TOOL_OPTIONS")
            .env_remove("_JAVA_OPTIONS")
            .env_remove("JDK_JAVA_OPTIONS")
            .stdin(Stdio::piped());
        if hanging {
            let mut path = vec![p.join("jdk/bin")];
            if let Some(inherited) = std::env::var_os("PATH") {
                path.extend(std::env::split_paths(&inherited));
            }
            cmd.env("JAVA_HOME", p.join("jdk"))
                .env("PATH", std::env::join_paths(path).unwrap())
                .env("SHIELD_NATIVE_JAVA", &self.java)
                .env("SHIELD_NATIVE_KEYTOOL", &self.keytool)
                .env("SHIELD_NATIVE_GATE", p);
            if let Some(home) = std::env::var_os("JAVA_HOME") {
                cmd.env("SHIELD_NATIVE_JAVA_HOME", home);
            } else {
                cmd.env_remove("SHIELD_NATIVE_JAVA_HOME");
            }
        }
        cmd
    }
    fn mcp(&self, hanging: bool, timeout: u64) -> Command {
        let p = self.root.path();
        let mut cmd = self.command(hanging);
        cmd.args(["mcp", "serve", "--input-root"])
            .arg(p.join("input"))
            .arg("--output-root")
            .arg(p.join("output"))
            .args([
                "--use-shared-settings",
                "--allow-execution",
                "--allow-certificate",
            ])
            .arg(&self.certificate)
            .arg("--timeout-seconds")
            .arg(timeout.to_string());
        cmd
    }
    fn check_unchanged(&self) {
        let p = self.root.path();
        assert_eq!(fs::read(p.join("input/app.apk")).unwrap(), self.source);
        assert!(fs::read_dir(p.join("output")).unwrap().all(|e| !e
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".mocika-")));
    }
    fn stopped(&mut self) {
        let p = self.root.path();
        for name in ["heartbeat", "child-heartbeat"] {
            let before = fs::read(p.join(name)).unwrap();
            assert!(!before.is_empty());
            std::thread::sleep(Duration::from_millis(100));
            assert_eq!(fs::read(p.join(name)).unwrap(), before, "后代未停止");
        }
        assert!(!p.join("output/signed.apk").exists());
        self.check_unchanged();
        self.root.disable_cleanup(false);
    }
}
fn wait_for(mut ready: impl FnMut() -> bool, deadline: Instant) {
    while !ready() {
        assert!(Instant::now() < deadline, "原生入口未在期限内完成");
        std::thread::sleep(Duration::from_millis(10));
    }
}
fn ready(root: &Path) -> bool {
    ["heartbeat", "child-heartbeat"]
        .iter()
        .all(|n| root.join(n).metadata().is_ok_and(|m| m.len() > 0))
}
struct Guard(Child);
impl Drop for Guard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
#[ignore = "显式使用自有 APK、真实 JDK 和签名工具，验证完整 MCP 标准流签名"]
fn 真实共享证书发现签名验签和后续查询共用同一连接() {
    let f = Fixture::new();
    let mut session = client::Session::start(f.mcp(false, 120));
    session.send(json!({"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}));
    let list = session.receive(2);
    assert!(list["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .any(|v| v["name"] == "shield_sign"));
    let result = session.call(
        3,
        "shield_sign",
        json!({"input":"app.apk","output":"signed.apk"}),
    );
    assert_eq!(result["status"], "succeeded", "{result}");
    assert_eq!(result["cleanup_state"], "complete");
    assert_eq!(result["directory_synced"], !cfg!(windows));
    assert!(!result.to_string().contains(SECRET));
    assert!(!result.to_string().contains(f.root.path().to_str().unwrap()));
    let signer = PathBuf::from(std::env::var_os("SHIELD_TEST_APKSIGNER").unwrap());
    let signed = f.root.path().join("output/signed.apk");
    // 独立原工具的旧 argv 不具备中文兼容性；核对原字节的 ASCII 副本，而非改业务路径。
    let independent = tempfile::tempdir().unwrap();
    assert!(independent.path().as_os_str().is_ascii());
    let verified = independent.path().join("signed.apk");
    fs::copy(&signed, &verified).unwrap();
    assert_eq!(fs::read(&signed).unwrap(), fs::read(&verified).unwrap());
    assert!(shield_core::utils::java_command(&f.java)
        .arg("-jar")
        .arg(signer)
        .arg("verify")
        .arg(verified)
        .output()
        .unwrap()
        .status
        .success());
    assert_eq!(
        session.call(4, "shield_settings", json!({}))["status"],
        "ready"
    );
    session.close();
    f.check_unchanged();
}

#[test]
#[ignore = "需要真实 DEX APK、标准壳资源和 SDK；仅静态核验完整受控加固，不安装设备"]
fn 真实共享规划加固签名与绑定验收保持同一会话() {
    assert!(std::env::var_os("SHIELD_TEST_STANDARD_RESOURCES").is_some());
    let f = Fixture::new();
    let mut session = client::Session::start(f.mcp(false, 120));
    let seed = session.call(
        2,
        "shield_sign",
        json!({"input":"app.apk","output":"seed.apk"}),
    );
    assert_eq!(seed["status"], "succeeded", "{seed}");
    let p = f.root.path();
    fs::copy(p.join("output/seed.apk"), p.join("input/seed.apk")).unwrap();
    let source = fs::read(p.join("input/seed.apk")).unwrap();
    let plan = session.call(
        3,
        "shield_plan",
        json!({"input":"seed.apk","output":"protected.apk","certificate_id":f.certificate}),
    );
    assert!(
        matches!(plan["status"].as_str(), Some("ready" | "warning")),
        "{plan}"
    );
    let result = session.call(
        4,
        "shield_protect",
        json!({"plan_id":plan["plan_id"],"expect_plan_sha256":plan["plan"]["sha256"]}),
    );
    assert_eq!(result["status"], "succeeded", "{result}");
    assert_eq!(result["cleanup_state"], "complete");
    assert!(!result.to_string().contains(SECRET));
    assert!(!result.to_string().contains(p.to_str().unwrap()));
    let final_path = p.join("output/protected.apk");
    let binding = shield_core::inspect_signing_input(&final_path, || Ok(()))
        .unwrap()
        .certificate_binding
        .unwrap();
    let fingerprint = binding
        .iter()
        .map(|byte| format!("{byte:02X}"))
        .collect::<String>();
    // 独立原工具只用 ASCII 路径对照，不能把未切换的旧中文 argv 当作新适配的失败。
    let independent = tempfile::tempdir().unwrap();
    let copy = independent.path().join("verified.apk");
    fs::copy(&final_path, &copy).unwrap();
    let signer = std::env::var_os("SHIELD_TEST_APKSIGNER").unwrap();
    let output = shield_core::utils::java_command(&f.java)
        .arg("-jar")
        .arg(signer)
        .args(["verify", "--print-certs"])
        .arg(&copy)
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(String::from_utf8(output.stdout)
        .unwrap()
        .to_ascii_uppercase()
        .contains(&fingerprint));
    assert!(shield_core::utils::no_window_command(
        std::env::var_os("SHIELD_TEST_ZIPALIGN").unwrap()
    )
    .args(["-c", "-P", "16", "4"])
    .arg(&copy)
    .output()
    .unwrap()
    .status
    .success());
    assert_eq!(fs::read(p.join("input/seed.apk")).unwrap(), source);
    assert_eq!(
        session.call(5, "shield_settings", json!({}))["status"],
        "ready"
    );
    session.close();
    f.check_unchanged();
}

#[test]
#[ignore = "真实原生工具代理挂起签名步骤，验证完整 CLI 控制及后代清场"]
fn 受控签名执行期间取消超时和输入断开都不交付() {
    for mode in ["cancel", "timeout", "closed"] {
        let mut f = Fixture::new();
        f.native_tool();
        f.root.disable_cleanup(true);
        let p = f.root.path();
        let timeout = if mode == "timeout" { 30 } else { 120 };
        let mut cmd = f.command(true);
        cmd.args(["agent", "sign", "--timeout-seconds"])
            .arg(timeout.to_string())
            .arg("-i")
            .arg(p.join("input/app.apk"))
            .arg("-o")
            .arg(p.join("output/signed.apk"))
            .arg("--control-stdin")
            .stdout(fs::File::create(p.join("events")).unwrap())
            .stderr(fs::File::create(p.join("errors")).unwrap());
        let mut child = Guard(cmd.spawn().unwrap());
        let deadline = Instant::now() + Duration::from_secs(timeout + 15);
        wait_for(
            || ready(p) || child.0.try_wait().unwrap().is_some(),
            deadline,
        );
        assert!(
            ready(p),
            "签名工具尚未开始：{}",
            fs::read_to_string(p.join("events")).unwrap()
        );
        let mut control = child.0.stdin.take().unwrap();
        match mode {
            "cancel" => control.write_all(b"{\"type\":\"cancel\"}\n").unwrap(),
            "closed" => drop(control),
            _ => {}
        }
        wait_for(|| child.0.try_wait().unwrap().is_some(), deadline);
        let status = child.0.wait().unwrap();
        assert_eq!(
            status.code(),
            Some(if mode == "timeout" { 124 } else { 130 })
        );
        assert!(fs::read(p.join("errors")).unwrap().is_empty());
        let events = fs::read_to_string(p.join("events")).unwrap();
        assert!(!events.contains(SECRET));
        let values: Vec<Value> = events
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();
        assert_eq!(values.iter().filter(|v| v["type"] == "result").count(), 1);
        for (index, value) in values.iter().enumerate() {
            assert_eq!(value["sequence"], index);
        }
        let last = values.last().unwrap();
        assert_eq!(
            last["code"],
            match mode {
                "timeout" => "TASK_TIMEOUT",
                "closed" => "CLIENT_DISCONNECTED",
                _ => "TASK_CANCELLED",
            }
        );
        assert_eq!(last["output_state"], "not_published");
        assert_eq!(last["cleanup_state"], "complete");
        f.stopped();
    }
}

#[test]
#[ignore = "真实原生工具代理挂起签名步骤，验证 MCP 断开与超时后的工作组停止"]
fn 同一连接签名超时不锁死且连接断开也停止后代() {
    for mode in ["timeout", "closed"] {
        let mut f = Fixture::new();
        f.native_tool();
        f.root.disable_cleanup(true);
        let mut session =
            client::Session::start(f.mcp(true, if mode == "timeout" { 30 } else { 120 }));
        session.send(json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"shield_sign","arguments":{"input":"app.apk","output":"signed.apk"}}}));
        wait_for(
            || ready(f.root.path()),
            Instant::now() + Duration::from_secs(25),
        );
        if mode == "timeout" {
            let response = session.receive(2);
            let result = &response["result"]["structuredContent"];
            assert_eq!(result["code"], "TASK_TIMEOUT", "{result}");
            assert_eq!(result["cleanup_state"], "complete");
            assert_eq!(result["session_execution_blocked"], false);
            assert_eq!(
                session.call(3, "shield_settings", json!({}))["status"],
                "ready"
            );
        }
        session.close();
        f.stopped();
    }
}

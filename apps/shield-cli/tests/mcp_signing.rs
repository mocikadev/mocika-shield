//! MCP 单次独立签名；所有资料隔离，不修改真实宿主配置或证书库。
#![cfg(all(feature = "mcp-server", target_os = "macos"))]
use serde_json::{json, Value};
use shield_services::{
    certificates::{CertificateStoreState, CertificateUpsertInput},
    paths::ApplicationPaths,
};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};
#[path = "support/mcp_client.rs"]
mod client;
use client::Session;
#[path = "mcp_signing/certificates.rs"]
mod certificates;
#[path = "mcp_signing/real.rs"]
mod real;

const SECRET: &str = "mcp-signing-test-password";
struct Fixture {
    root: tempfile::TempDir,
    cli: PathBuf,
    package_files: Vec<(PathBuf, Vec<u8>)>,
}
impl Fixture {
    fn new() -> Self {
        let root = tempfile::Builder::new()
            .prefix("单次签名 中文空格")
            .tempdir()
            .unwrap();
        for name in ["输入", "输出"] {
            fs::create_dir(root.path().join(name)).unwrap();
        }
        fs::write(root.path().join("输入/应用.apk"), b"fixture").unwrap();
        let signer = root.path().join("签名工具.jar");
        fs::write(&signer, b"fixture").unwrap();
        fs::write(
            root.path().join("shield-cli.toml"),
            format!(
                "schema_version=1\n[sign]\napksigner={}\n",
                serde_json::to_string(signer.to_str().unwrap()).unwrap()
            ),
        )
        .unwrap();
        let mut cli = PathBuf::from(env!("CARGO_BIN_EXE_shield"));
        let mut package_files = Vec::new();
        if let Some(source) = std::env::var_os("SHIELD_SHARED_TEST_APP") {
            let source = PathBuf::from(source);
            assert!(source.is_absolute() && source.is_dir());
            let app = root.path().join("独立 签名应用.app");
            assert!(!app.exists());
            assert!(Command::new("ditto")
                .arg(source)
                .arg(&app)
                .status()
                .unwrap()
                .success());
            assert!(!app.join("Contents/Resources/skills").exists());
            for name in [
                "Contents/MacOS/shield",
                "Contents/MacOS/mocika-shield",
                "Contents/Resources/tools/apktool.jar",
                "Contents/Resources/tools/apksigner.jar",
                "Contents/Resources/resources/resources.zip",
                "Contents/Resources/resources/resources-api19.zip",
                "Contents/_CodeSignature/CodeResources",
            ] {
                let path = app.join(name);
                package_files.push((path.clone(), package_hash(&path)));
            }
            cli = app.join("Contents/MacOS/shield");
        }
        Self {
            root,
            cli,
            package_files,
        }
    }
    fn home(&self) -> PathBuf {
        self.root.path().join("共享资料")
    }
    fn store(&self) -> CertificateStoreState {
        let paths = ApplicationPaths::isolated(&self.home()).unwrap();
        CertificateStoreState::initialize(paths.certificate_database(), paths.keystore_dir())
            .unwrap()
    }
    fn cli_command(&self) -> Command {
        let mut cmd = shield_core::utils::no_window_command(&self.cli);
        cmd.current_dir(self.root.path());
        // 完整包验收不覆盖工具路径，实际执行必须自动找到包内 JAR。
        if self.package_files.is_empty() {
            cmd.arg("--config")
                .arg(self.root.path().join("shield-cli.toml"));
        }
        cmd
    }
    fn command(&self, ids: &[&str], execute: bool, shared: bool, timeout: u64) -> Command {
        let mut cmd = self.cli_command();
        if shared {
            cmd.arg("--app-home").arg(self.home());
        }
        cmd.args(["mcp", "serve", "--input-root"])
            .arg(self.root.path().join("输入"))
            .arg("--output-root")
            .arg(self.root.path().join("输出"))
            .arg("--timeout-seconds")
            .arg(timeout.to_string())
            .env("MOCIKA_SHIELD_KS_PASS", "inherited-not-a-credential")
            .env("MOCIKA_SHIELD_KEY_PASS", "inherited-key-not-a-credential")
            // 模拟当前 Agent 的精简环境：能查询证书不代表能够解密签名资料。
            .env_remove("USER")
            .env_remove("USERNAME")
            .env_remove("JAVA_TOOL_OPTIONS")
            .env_remove("_JAVA_OPTIONS")
            .env_remove("JDK_JAVA_OPTIONS");
        if shared {
            cmd.arg("--use-shared-settings");
            for id in ids {
                cmd.arg(format!("--allow-certificate={id}"));
            }
        }
        if execute {
            cmd.arg("--allow-execution");
        }
        cmd
    }
    fn fake(&self, store: &CertificateStoreState, name: &str, v4: bool) -> String {
        let ks = self.root.path().join(format!("{name}.p12"));
        fs::write(&ks, b"fixture").unwrap();
        let mut input = draft(name);
        input.sign_v4 = v4;
        store
            .save_certificate(&input, ks.to_str().unwrap(), "中文别名", None)
            .unwrap()
            .id
    }
    fn unchanged_tasks(&self) {
        assert!(fs::read_dir(self.root.path().join("输出"))
            .unwrap()
            .all(|e| !e
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".mocika-")));
        for (path, hash) in &self.package_files {
            assert_eq!(&package_hash(path), hash, "签名不得改写应用包");
        }
        if !self.package_files.is_empty() {
            let app = self
                .cli
                .parent()
                .unwrap()
                .parent()
                .unwrap()
                .parent()
                .unwrap();
            assert!(Command::new("codesign")
                .args(["--verify", "--deep", "--strict"])
                .arg(app)
                .status()
                .unwrap()
                .success());
        }
    }
}
fn package_hash(path: &Path) -> Vec<u8> {
    use sha2::{Digest, Sha256};
    Sha256::digest(fs::read(path).unwrap()).to_vec()
}
impl Drop for Fixture {
    fn drop(&mut self) {
        if std::thread::panicking() {
            self.root.disable_cleanup(true);
        }
    }
}
fn draft(name: &str) -> CertificateUpsertInput {
    CertificateUpsertInput {
        id: None,
        name: name.into(),
        source_type: "external".into(),
        keystore_path: String::new(),
        keystore_password: SECRET.into(),
        key_alias: "中文别名".into(),
        key_password: String::new(),
        ks_type: Some("PKCS12".into()),
        sign_v1: true,
        sign_v2: true,
        sign_v3: true,
        sign_v4: false,
        auto_sign_enabled: false,
        note: "私有备注".into(),
        set_as_default: true,
        copy_keystore_to_managed: false,
        managed_file_name: None,
    }
}
fn private(value: &Value, fixture: &Fixture) {
    let text = value.to_string();
    for secret in [
        SECRET,
        "inherited-not-a-credential",
        "inherited-key-not-a-credential",
        "中文别名",
        "私有备注",
        fixture.root.path().to_str().unwrap(),
        "shared_binding",
    ] {
        assert!(!text.contains(secret), "结果泄漏私有字段");
    }
}
#[test]
fn 只在既有共享执行模式注册一个签名工具而无额外前置参数() {
    let fixture = Fixture::new();
    for (execute, shared, expected) in [(false, true, 5), (true, false, 5), (true, true, 15)] {
        let mut session = Session::start(fixture.command(&["allowed"], execute, shared, 20));
        session.send(json!({"jsonrpc":"2.0","id":2,"method":"tools/list"}));
        let tools = session.receive(2)["result"]["tools"]
            .as_array()
            .unwrap()
            .clone();
        assert_eq!(tools.len(), expected);
        let signing = tools.iter().find(|t| t["name"] == "shield_sign");
        assert_eq!(signing.is_some(), execute && shared);
        if let Some(signing) = signing {
            assert_eq!(signing["annotations"]["readOnlyHint"], false);
            assert_eq!(signing["annotations"]["destructiveHint"], false);
            assert_eq!(
                signing["inputSchema"]["required"],
                json!(["input", "output"])
            );
            assert_eq!(
                signing["inputSchema"]["properties"]
                    .as_object()
                    .unwrap()
                    .len(),
                3
            );
        } else {
            assert_eq!(
                session.call(3, "shield_sign", json!({}))["code"],
                "TOOL_NOT_AVAILABLE"
            );
        }
        session.close();
    }
    assert!(!fixture.home().exists());
}
#[test]
fn 已确认清场的普通规划错误不应锁死会话或要求重启() {
    let fixture = Fixture::new();
    let store = fixture.store();
    let id = fixture.fake(&store, "版本门禁", true);
    let mut session = Session::start(fixture.command(&[&id], true, true, 20));
    for call in [2, 3] {
        let value = session.call(
            call,
            "shield_sign",
            json!({"input":"应用.apk","output":"输出.apk"}),
        );
        assert_eq!(value["code"], "SIGNING_V4_NOT_SUPPORTED");
        assert_eq!(value["cleanup_state"], "complete");
        assert_eq!(value["session_execution_blocked"], false);
        private(&value, &fixture);
    }
    session.close();
    fixture.unchanged_tasks();
    assert!(!fixture.root.path().join("输出/输出.apk").exists());
}

#[test]
fn 参数路径和证书越界被拒绝且不能注入计划或秘密() {
    let fixture = Fixture::new();
    let mut session = Session::start(fixture.command(&["allowed"], true, true, 20));
    for (extra, value, code) in [
        ("certificate_id", json!("hidden"), "CERTIFICATE_NOT_ALLOWED"),
        ("input", json!("../应用.apk"), "PATH_NOT_ALLOWED"),
        ("output", json!("../输出.apk"), "PATH_NOT_ALLOWED"),
        ("plan_id", json!("a".repeat(32)), "ARGUMENT_INVALID"),
        (
            "expect_plan_sha256",
            json!("b".repeat(64)),
            "ARGUMENT_INVALID",
        ),
        ("key_alias", json!("中文别名"), "ARGUMENT_INVALID"),
        ("password", json!(SECRET), "ARGUMENT_INVALID"),
        ("runtime_profile", json!("standard"), "ARGUMENT_INVALID"),
    ] {
        let mut args = json!({"input":"应用.apk","output":"输出.apk"});
        args[extra] = value;
        let result = session.call(2, "shield_sign", args);
        assert_eq!(result["code"], code);
        private(&result, &fixture);
    }
    std::os::unix::fs::symlink(
        fixture.root.path().join("输入/应用.apk"),
        fixture.root.path().join("输入/链接.apk"),
    )
    .unwrap();
    assert_eq!(
        session.call(
            3,
            "shield_sign",
            json!({"input":"链接.apk","output":"输出.apk"})
        )["code"],
        "PATH_NOT_ALLOWED"
    );
    fs::write(fixture.root.path().join("输出/已存在.apk"), b"original").unwrap();
    assert_eq!(
        session.call(
            4,
            "shield_sign",
            json!({"input":"应用.apk","output":"已存在.apk"})
        )["code"],
        "OUTPUT_EXISTS"
    );
    session.close();
    assert!(!fixture.home().exists());
    fixture.unchanged_tasks();
    assert_eq!(
        fs::read(fixture.root.path().join("输出/已存在.apk")).unwrap(),
        b"original"
    );
}
#[test]
fn 多项范围按共享默认选择且歧义不猜选也不锁会话() {
    let fixture = Fixture::new();
    let store = fixture.store();
    let first = fixture.fake(&store, "第一项", true);
    let second = fixture.fake(&store, "默认项", true);
    let mut session = Session::start(fixture.command(&[&first, &second], true, true, 30));
    let value = session.call(
        2,
        "shield_sign",
        json!({"input":"应用.apk","output":"输出.apk"}),
    );
    assert_eq!(value["code"], "SIGNING_V4_NOT_SUPPORTED");
    assert_eq!(value["session_execution_blocked"], false);
    let _outside = fixture.fake(&store, "范围外默认", true);
    let value = session.call(
        3,
        "shield_sign",
        json!({"input":"应用.apk","output":"输出.apk"}),
    );
    assert_eq!(value["code"], "CERTIFICATE_REQUIRED");
    assert_eq!(value["cleanup_state"], "not_needed");
    assert_eq!(value["session_execution_blocked"], false);
    let value = session.call(
        4,
        "shield_sign",
        json!({"input":"应用.apk","output":"输出.apk","certificate_id":first}),
    );
    assert_eq!(value["code"], "SIGNING_V4_NOT_SUPPORTED");
    private(&value, &fixture);
    assert_eq!(
        session.call(
            5,
            "shield_execution_status",
            json!({"plan_id":value["plan_id"]})
        )["operation"],
        "agent_sign"
    );
    assert_eq!(
        session.call(
            6,
            "shield_protect",
            json!({"plan_id":value["plan_id"],"expect_plan_sha256":"a".repeat(64)})
        )["code"],
        "PLAN_NOT_AVAILABLE"
    );
    session.close();
    fixture.unchanged_tasks();
}
#[test]
fn 唯一允许证书缺失时返回错误且不创建资料库() {
    let fixture = Fixture::new();
    let mut session = Session::start(fixture.command(&["missing"], true, true, 20));
    let value = session.call(
        2,
        "shield_sign",
        json!({"input":"应用.apk","output":"输出.apk"}),
    );
    assert_eq!(value["code"], "CREDENTIAL_UNAVAILABLE");
    assert_eq!(value["session_execution_blocked"], false);
    session.close();
    assert!(!fixture.home().exists());
    fixture.unchanged_tasks();
}

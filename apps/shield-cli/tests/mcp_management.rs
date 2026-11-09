//! 共享管理的真实标准流验收；仅使用合成资料，不接触业务库、密码或系统 Java。
#![cfg(all(feature = "mcp-server", target_os = "macos"))]
use serde_json::{json, Value};
use shield_app::{
    certificates::{CertificateStoreState, CertificateUpsertInput},
    config::ConfigStore,
    paths::ApplicationPaths,
};
use std::{fs, path::PathBuf, process::Command};
#[path = "support/mcp_client.rs"]
mod client;
use client::Session;

struct Fixture {
    root: tempfile::TempDir,
}
impl Fixture {
    fn new() -> Self {
        let root = tempfile::Builder::new()
            .prefix("MCP 管理 中文空格")
            .tempdir()
            .unwrap();
        for name in ["输入", "输出"] {
            fs::create_dir(root.path().join(name)).unwrap();
        }
        fs::write(root.path().join("项目.toml"), "schema_version=1\n").unwrap();
        Self { root }
    }
    fn home(&self) -> PathBuf {
        self.root.path().join("资料")
    }
    fn paths(&self) -> ApplicationPaths {
        ApplicationPaths::isolated(&self.home()).unwrap()
    }
    fn store(&self) -> CertificateStoreState {
        let paths = self.paths();
        CertificateStoreState::initialize(paths.certificate_database(), paths.keystore_dir())
            .unwrap()
    }
    fn command(&self, shared: bool, execute: bool, ids: &[&str]) -> Command {
        let mut cmd = shield_core::utils::no_window_command(env!("CARGO_BIN_EXE_shield"));
        cmd.arg("--config").arg(self.root.path().join("项目.toml"));
        if shared {
            cmd.arg("--app-home").arg(self.home());
        }
        cmd.args(["mcp", "serve", "--input-root"])
            .arg(self.root.path().join("输入"))
            .arg("--output-root")
            .arg(self.root.path().join("输出"))
            .args(["--timeout-seconds", "120"])
            .env_remove("USER")
            .env_remove("USERNAME")
            .env("MOCIKA_SHIELD_KS_PASS", "不得使用的继承口令");
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
    fn certificate(&self, store: &CertificateStoreState, name: &str) -> String {
        let file = self.root.path().join(format!("{name}.p12"));
        fs::write(&file, b"fixture-not-keystore").unwrap();
        store
            .save_certificate(
                &CertificateUpsertInput {
                    id: None,
                    name: name.into(),
                    source_type: "external".into(),
                    keystore_path: String::new(),
                    keystore_password: "synthetic-management-secret".into(),
                    key_alias: "private-alias".into(),
                    key_password: String::new(),
                    ks_type: Some("PKCS12".into()),
                    sign_v1: true,
                    sign_v2: true,
                    sign_v3: true,
                    sign_v4: false,
                    auto_sign_enabled: true,
                    note: "private-note".into(),
                    set_as_default: true,
                    copy_keystore_to_managed: false,
                    managed_file_name: None,
                },
                file.to_str().unwrap(),
                "private-alias",
                None,
            )
            .unwrap()
            .id
    }
    fn private(&self, value: &Value) {
        let text = value.to_string();
        for secret in [
            "synthetic-management-secret",
            "不得使用的继承口令",
            "private-alias",
            "private-note",
            "private-path",
            self.root.path().to_str().unwrap(),
        ] {
            assert!(!text.contains(secret));
        }
    }
}
#[test]
fn 工具沿用共享和执行开关且只读查询不创建资料() {
    let fixture = Fixture::new();
    for (shared, execute, count) in [
        (false, false, 3),
        (false, true, 5),
        (true, false, 5),
        (true, true, 11),
    ] {
        let mut session = Session::start(fixture.command(shared, execute, &["allowed"]));
        session.send(json!({"jsonrpc":"2.0","id":2,"method":"tools/list"}));
        let list = session.receive(2)["result"]["tools"]
            .as_array()
            .unwrap()
            .clone();
        assert_eq!(list.len(), count);
        for name in [
            "shield_update_settings",
            "shield_update_certificate",
            "shield_set_default_certificate",
        ] {
            let tool = list.iter().find(|tool| tool["name"] == name);
            assert_eq!(tool.is_some(), shared && execute);
            if let Some(tool) = tool {
                assert_eq!(tool["annotations"]["readOnlyHint"], false);
                assert_eq!(tool["annotations"]["destructiveHint"], true);
            } else {
                assert_eq!(
                    session.call(3, name, json!({}))["code"],
                    "TOOL_NOT_AVAILABLE"
                );
            }
        }
        if shared {
            let value = session.call(4, "shield_settings", json!({}));
            assert_eq!(value["status"], "ready");
            fixture.private(&value);
        }
        session.close();
        assert!(!fixture.home().exists());
    }
}
#[test]
fn 资料字段更新默认切换及恢复不改证书材料或其他配置() {
    let fixture = Fixture::new();
    let store = fixture.store();
    let first = fixture.certificate(&store, "private-first");
    let second = fixture.certificate(&store, "private-second");
    let hidden = fixture.certificate(&store, "private-hidden");
    let paths = fixture.paths();
    fs::create_dir_all(paths.config_file().parent().unwrap()).unwrap();
    fs::write(paths.config_file(),format!("theme_mode='dark'\n[telemetry]\nenabled=false\n[unknown]\nkeep='yes'\n[protect_defaults]\nruntime_mode='standard'\nenvironment_policy='compatible'\ncertificate_id='{hidden}'\noutput_directory_mode='fixed'\nfixed_output_directory='private-path'\n")).unwrap();
    let original = store.get_certificate(&first).unwrap().unwrap();
    let material = fs::read(&original.keystore_path).unwrap();
    let mut session = Session::start(fixture.command(true, true, &[&first, &second]));
    let value = session.call(2, "shield_settings", json!({}));
    assert_eq!(
        value["data"]["protect_defaults"]["certificate_scope"],
        "outside_scope"
    );
    assert!(!value.to_string().contains(&hidden));
    fixture.private(&value);
    for args in [
        json!({}),
        json!({"runtime_mode":null,"environment_policy":"strict"}),
        json!({"certificate_id":hidden}),
        json!({"database":"private-path"}),
    ] {
        let value = session.call(3, "shield_update_settings", args);
        assert_eq!(value["change_state"], "not_attempted");
        assert_eq!(value["session_execution_blocked"], false);
    }
    let value=session.call(4,"shield_update_settings",json!({"runtime_mode":"android_api19","environment_policy":"strict","sign_after_protect":false,"certificate_id":first}));
    assert_eq!(value["status"], "ready");
    assert_eq!(value["change_state"], "confirmed");
    fixture.private(&value);
    let value=session.call(5,"shield_update_certificate",json!({"certificate_id":first,"name":"--help","note":"中文备注; $(不执行)","v3":false,"auto_sign":false}));
    assert_eq!(value["status"], "ready");
    assert!(!value.to_string().contains("不执行"));
    fixture.private(&value);
    let value = session.call(
        6,
        "shield_set_default_certificate",
        json!({"certificate_id":second}),
    );
    assert_eq!(value["status"], "ready");
    assert_eq!(value["data"]["default_certificate_id"], second);
    assert_eq!(
        ConfigStore::new(paths.config_file())
            .read_protect_defaults()
            .unwrap()
            .certificate_id
            .as_deref(),
        Some(first.as_str())
    );
    let current = store.get_certificate(&first).unwrap().unwrap();
    assert_eq!(current.name, "--help");
    assert_eq!(current.note, "中文备注; $(不执行)");
    assert_eq!(current.keystore_path, original.keystore_path);
    assert_eq!(current.key_alias, original.key_alias);
    assert!(current.keystore_password == original.keystore_password);
    assert_eq!(fs::read(&current.keystore_path).unwrap(), material);
    assert!(!current.sign_v3 && !current.auto_sign_enabled);
    let config: toml::Value =
        toml::from_str(&fs::read_to_string(paths.config_file()).unwrap()).unwrap();
    assert_eq!(config["unknown"]["keep"].as_str(), Some("yes"));
    assert_eq!(config["theme_mode"].as_str(), Some("dark"));
    assert_eq!(config["telemetry"]["enabled"].as_bool(), Some(false));
    assert_eq!(
        config["protect_defaults"]["fixed_output_directory"].as_str(),
        Some("private-path")
    );
    let restored=session.call(7,"shield_update_settings",json!({"runtime_mode":"standard","environment_policy":"compatible","sign_after_protect":null,"certificate_id":null}));
    assert_eq!(restored["status"], "ready");
    assert_eq!(
        restored["data"]["protect_defaults"]["certificate_scope"],
        "inherit_default"
    );
    let value=session.call(8,"shield_update_certificate",json!({"certificate_id":first,"v3":true,"auto_sign":true,"name":"private-first","note":"private-note"}));
    assert_eq!(value["status"], "ready");
    assert!(fs::read_dir(fixture.root.path().join("输出"))
        .unwrap()
        .next()
        .is_none());
    session.close();
}

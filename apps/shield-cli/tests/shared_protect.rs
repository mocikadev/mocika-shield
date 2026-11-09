//! 普通加固的共享来源与产物回归；不等同于受控 Agent 或跨平台设备验收。
use serde_json::{json, Value};
use shield_app::{
    certificates::{CertificateStoreState, CertificateUpsertInput},
    config::ConfigStore,
    paths::ApplicationPaths,
};
use std::{
    fs,
    path::Path,
    process::{Command, Output},
};

const SECRET: &str = "protect-shared-test-password";
fn command(root: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_shield"));
    command
        .arg("--app-home")
        .arg(root)
        .arg("protect")
        .arg("--json")
        .env_remove("MOCIKA_SHIELD_KS_PASS")
        .env_remove("MOCIKA_SHIELD_KEY_PASS");
    command
}
fn events(output: Output, exit: i32) -> Vec<Value> {
    assert_eq!(output.status.code(), Some(exit));
    assert!(output.stderr.is_empty());
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(!text.contains(SECRET));
    text.lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}
fn fixture(root: &Path, auto: bool, sign: Option<bool>) -> String {
    let paths = ApplicationPaths::isolated(root).unwrap();
    let store =
        CertificateStoreState::initialize(paths.certificate_database(), paths.keystore_dir())
            .unwrap();
    let certificate = store
        .save_certificate(
            &CertificateUpsertInput {
                id: None,
                name: "隔离证书".into(),
                source_type: "external".into(),
                keystore_path: "/fixture.p12".into(),
                keystore_password: SECRET.into(),
                key_alias: "release".into(),
                key_password: SECRET.into(),
                ks_type: Some("PKCS12".into()),
                sign_v1: true,
                sign_v2: true,
                sign_v3: true,
                sign_v4: false,
                auto_sign_enabled: auto,
                note: String::new(),
                set_as_default: true,
                copy_keystore_to_managed: false,
                managed_file_name: None,
            },
            "/fixture.p12",
            "release",
            None,
        )
        .unwrap();
    ConfigStore::new(paths.config_file())
        .update_protect_defaults(
            serde_json::from_value(json!({"sign_after_protect": sign})).unwrap(),
        )
        .unwrap();
    certificate.id
}

#[test]
fn 关闭签名后的显式版本和证书冲突不得被忽略() {
    let temp = tempfile::tempdir().unwrap();
    let output = command(temp.path())
        .args(["--sign-after-protect", "false", "--use-default-certificate"])
        .output()
        .unwrap();
    assert!(events(output, 1)[0]["message"]
        .as_str()
        .unwrap()
        .contains("不能同时关闭"));
    let output = command(temp.path())
        .args(["--sign-after-protect", "false", "--v3", "false"])
        .output()
        .unwrap();
    assert!(events(output, 1)[0]["message"]
        .as_str()
        .unwrap()
        .contains("不能忽略"));
    assert!(!temp.path().join("data").exists());
}

#[test]
fn 已有输出不覆盖且签名凭据不混用也不回填() {
    let temp = tempfile::tempdir().unwrap();
    fixture(temp.path(), true, None);
    let output = command(temp.path())
        .env("MOCIKA_SHIELD_KS_PASS", SECRET)
        .args(["-i", "input.apk", "-o", "output.apk"])
        .output()
        .unwrap();
    assert!(events(output, 1)[0]["message"]
        .as_str()
        .unwrap()
        .contains("不能混用"));
    let target = temp.path().join("existing.apk");
    fs::write(&target, b"original").unwrap();
    let output = command(temp.path())
        .args(["--sign-after-protect", "false", "-i", "input.apk", "-o"])
        .arg(&target)
        .output()
        .unwrap();
    assert!(events(output, 1)[0]["message"]
        .as_str()
        .unwrap()
        .contains("输出已存在"));
    assert_eq!(fs::read(target).unwrap(), b"original");
    assert!(!temp.path().join("output.apk").exists());
}

#[test]
fn 沿用默认签名偏好时坏编号不回退且显式关闭仍有效() {
    let temp = tempfile::tempdir().unwrap();
    let id = fixture(temp.path(), true, None);
    let output = command(temp.path())
        .args([
            "-i",
            "input.apk",
            "-o",
            "output.apk",
            "--certificate-id",
            "missing",
        ])
        .output()
        .unwrap();
    assert!(events(output, 1)[0]["message"]
        .as_str()
        .unwrap()
        .contains("不会改用其他"));
    let paths = ApplicationPaths::isolated(temp.path()).unwrap();
    ConfigStore::new(paths.config_file())
        .update_protect_defaults(serde_json::from_value(json!({"certificate_id":id})).unwrap())
        .unwrap();
    let output = command(temp.path())
        .args(["-i", "input.apk", "-o", "output.apk", "--v4", "true"])
        .output()
        .unwrap();
    assert!(events(output, 1)[0]["message"]
        .as_str()
        .unwrap()
        .contains("暂不支持 V4"));
}

#[test]
fn 项目材料与明确共享证书冲突且原配置不回显() {
    let temp = tempfile::tempdir().unwrap();
    let id = fixture(temp.path(), true, Some(true));
    let path = temp.path().join("shield-cli.toml");
    fs::write(
        &path,
        "schema_version=1\n[sign]\nkeystore='other.p12'\nkey_alias='another'\n",
    )
    .unwrap();
    let output = command(temp.path())
        .arg("--config")
        .arg(&path)
        .args([
            "-i",
            "input.apk",
            "-o",
            "output.apk",
            "--certificate-id",
            &id,
        ])
        .output()
        .unwrap();
    assert!(events(output, 1)[0]["message"]
        .as_str()
        .unwrap()
        .contains("不能与临时"));
    fs::write(
        &path,
        format!("schema_version=1\n[sign]\npassword='{SECRET}'\n"),
    )
    .unwrap();
    let output = command(temp.path())
        .arg("--config")
        .arg(&path)
        .output()
        .unwrap();
    assert!(events(output, 1)[0]["message"]
        .as_str()
        .unwrap()
        .contains("不会输出文件原文"));
}

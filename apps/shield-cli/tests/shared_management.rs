//! 本地资料管理的真实命令回归；所有上下文均为显式隔离目录。
use serde_json::{json, Value};
use shield_services::{
    certificates::{CertificatePreferencesPatch, CertificateStoreState, CertificateUpsertInput},
    paths::ApplicationPaths,
};
use std::{
    fs,
    path::Path,
    process::{Command, Output},
};

const SECRET: &str = "synthetic-management-password";
fn command(root: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_shield"));
    command
        .arg("--app-home")
        .arg(root)
        .env_remove("MOCIKA_SHIELD_KS_PASS")
        .env_remove("MOCIKA_SHIELD_KEY_PASS");
    command
}
fn response(output: Output, exit: i32) -> Value {
    assert_eq!(output.status.code(), Some(exit));
    assert!(output.stderr.is_empty(), "命令不应额外输出原始诊断");
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(!text.contains(SECRET));
    assert!(!text.contains("keystore_password"));
    assert!(!text.contains("key_password"));
    assert_eq!(text.lines().count(), 1);
    let value: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(value["schema_version"], 1);
    assert_eq!(value["tool_version"], env!("CARGO_PKG_VERSION"));
    assert_eq!(value["type"], if exit == 0 { "result" } else { "error" });
    value
}
fn run(root: &Path, args: &[&str]) -> Value {
    response(command(root).args(args).output().unwrap(), 0)
}
fn store(root: &Path) -> CertificateStoreState {
    let paths = ApplicationPaths::isolated(root).unwrap();
    CertificateStoreState::initialize(paths.certificate_database(), paths.keystore_dir()).unwrap()
}
fn seed(root: &Path) -> String {
    let file = root.join("外部 中文证书.p12");
    fs::write(&file, b"fixture-not-keystore").unwrap();
    store(root)
        .save_certificate(
            &CertificateUpsertInput {
                id: None,
                name: "原始名称".into(),
                source_type: "external".into(),
                keystore_path: file.to_str().unwrap().into(),
                keystore_password: SECRET.into(),
                key_alias: "中文Alias".into(),
                key_password: SECRET.into(),
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
            },
            file.to_str().unwrap(),
            "中文Alias",
            None,
        )
        .unwrap()
        .id
}

#[test]
fn 只读查询不创建应用目录且摘要缺失明确停止() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("尚未创建 中文空格");
    assert_eq!(
        run(&root, &["cert", "list"])["data"]["certificates"],
        json!([])
    );
    assert_eq!(
        run(&root, &["config", "show"])["data"]["protect_defaults"]["runtime_mode"],
        "standard"
    );
    let value = response(
        command(&root)
            .args(["cert", "show", "missing"])
            .output()
            .unwrap(),
        1,
    );
    assert_eq!(value["code"], "CERTIFICATE_NOT_FOUND");
    assert!(!root.exists());
}

#[test]
fn 无效上下文和不支持的项目配置不被静默忽略() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("未创建");
    let value = response(
        command(Path::new("relative"))
            .args(["cert", "list"])
            .output()
            .unwrap(),
        1,
    );
    assert_eq!(value["code"], "APP_HOME_INVALID");
    let value = response(
        command(&root)
            .args(["--config", "不存在.toml", "config", "show"])
            .output()
            .unwrap(),
        1,
    );
    assert_eq!(value["code"], "CONFIG_NOT_SUPPORTED");
    let value = response(
        command(&root)
            .args(["doctor", "--operation", "protect"])
            .output()
            .unwrap(),
        1,
    );
    assert_eq!(value["code"], "APP_HOME_NOT_SUPPORTED");
    assert!(!root.exists());
}

#[test]
fn 业务配置字段更新保留界面统计与未知字段且显式清空有效() {
    let temp = tempfile::tempdir().unwrap();
    let paths = ApplicationPaths::isolated(temp.path()).unwrap();
    fs::create_dir(paths.config_file().parent().unwrap()).unwrap();
    fs::write(paths.config_file(), "theme_mode='dark'\n[telemetry]\nenabled=false\nanonymous_id='private-fixture-id'\n[protect_defaults]\ncertificate_id='old'\n[future]\nunknown=42\n").unwrap();
    let value = run(
        temp.path(),
        &[
            "config",
            "set",
            "--environment-policy",
            "strict",
            "--sign-after-protect",
            "false",
        ],
    );
    assert_eq!(value["data"]["protect_defaults"]["certificate_id"], "old");
    assert_eq!(
        value["data"]["protect_defaults"]["sign_after_protect"],
        false
    );
    assert!(!value.to_string().contains("private-fixture-id"));
    let raw = fs::read_to_string(paths.config_file()).unwrap();
    let config: toml::Value = toml::from_str(&raw).unwrap();
    assert_eq!(config["theme_mode"].as_str(), Some("dark"));
    assert_eq!(config["telemetry"]["enabled"].as_bool(), Some(false));
    assert_eq!(config["future"]["unknown"].as_integer(), Some(42));
    let value = run(
        temp.path(),
        &[
            "config",
            "set",
            "--clear-certificate",
            "--inherit-auto-sign",
        ],
    );
    assert!(value["data"]["protect_defaults"]["certificate_id"].is_null());
    assert!(value["data"]["protect_defaults"]["sign_after_protect"].is_null());
    assert_eq!(
        value["data"]["protect_defaults"]["environment_policy"],
        "strict"
    );
    fs::write(paths.config_file(), "[broken").unwrap();
    let value = response(
        command(temp.path())
            .args(["config", "set", "--runtime-mode", "standard"])
            .output()
            .unwrap(),
        1,
    );
    assert_eq!(value["code"], "CONFIG_OPERATION_FAILED");
    assert_eq!(fs::read_to_string(paths.config_file()).unwrap(), "[broken");
}

#[test]
fn 共享服务与命令双向可见且编辑不改材料删除不删文件() {
    let temp = tempfile::tempdir().unwrap();
    let id = seed(temp.path());
    assert_eq!(
        run(temp.path(), &["cert", "show", &id])["data"]["certificate"]["key_alias"],
        "中文Alias"
    );
    run(
        temp.path(),
        &["cert", "edit", &id, "--name", "命令行改名", "--v3", "false"],
    );
    let service = store(temp.path());
    let value = service.get_certificate(&id).unwrap().unwrap();
    assert_eq!(value.name, "命令行改名");
    assert!(!value.sign_v3);
    assert_eq!(value.keystore_password, SECRET);
    service
        .patch_certificate_preferences(
            &id,
            CertificatePreferencesPatch {
                note: Some("另一入口更新".into()),
                ..Default::default()
            },
            false,
        )
        .unwrap();
    assert_eq!(
        run(temp.path(), &["cert", "show", &id])["data"]["certificate"]["note"],
        "另一入口更新"
    );
    run(temp.path(), &["cert", "clear-default"]);
    assert!(!service.get_certificate(&id).unwrap().unwrap().is_default);
    run(temp.path(), &["cert", "set-default", &id]);
    assert!(service.get_certificate(&id).unwrap().unwrap().is_default);
    let value = response(
        command(temp.path())
            .args(["cert", "remove", &id])
            .output()
            .unwrap(),
        1,
    );
    assert_eq!(value["code"], "CONFIRMATION_REQUIRED");
    assert_eq!(service.list_certificates().unwrap().len(), 1);
    run(temp.path(), &["cert", "remove", &id, "--yes"]);
    assert!(service.list_certificates().unwrap().is_empty());
    assert_eq!(
        fs::read(temp.path().join("外部 中文证书.p12")).unwrap(),
        b"fixture-not-keystore"
    );
}

#[test]
fn 缺密码和空补丁不创建库且新接口不接受密码参数() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("未创建");
    let value = response(
        command(&root)
            .args([
                "cert",
                "import",
                "--name",
                "演示",
                "--keystore",
                "不存在.p12",
            ])
            .output()
            .unwrap(),
        1,
    );
    assert_eq!(value["code"], "CREDENTIAL_REQUIRED");
    let value = response(command(&root).args(["config", "set"]).output().unwrap(), 1);
    assert_eq!(value["code"], "PATCH_REQUIRED");
    let value = response(
        command(&root)
            .args(["config", "set", "--certificate-id", "missing"])
            .output()
            .unwrap(),
        1,
    );
    assert_eq!(value["code"], "CERTIFICATE_NOT_FOUND");
    let output = command(&root)
        .args([
            "cert",
            "import",
            "--name",
            "演示",
            "--keystore",
            "不存在.p12",
            "--ks-pass",
            SECRET,
        ])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(!String::from_utf8_lossy(&output.stderr).contains(SECRET));
    assert!(!root.exists());
}

#[test]
fn 多个命令进程同时更新不同证书偏好和配置字段不会丢失() {
    let temp = tempfile::tempdir().unwrap();
    let id = seed(temp.path());
    run(
        temp.path(),
        &["config", "set", "--environment-policy", "compatible"],
    );
    std::thread::scope(|scope| {
        let workers: Vec<_> = [
            vec!["cert", "edit", id.as_str(), "--name", "并发名称"],
            vec!["cert", "edit", id.as_str(), "--note", "并发备注"],
            vec!["config", "set", "--runtime-mode", "android_api19"],
            vec!["config", "set", "--environment-policy", "strict"],
        ]
        .into_iter()
        .map(|args| {
            let root = temp.path();
            scope.spawn(move || run(root, &args))
        })
        .collect();
        for worker in workers {
            worker.join().unwrap();
        }
    });
    let summary = run(temp.path(), &["cert", "show", &id]);
    assert_eq!(summary["data"]["certificate"]["name"], "并发名称");
    assert_eq!(summary["data"]["certificate"]["note"], "并发备注");
    let config = run(temp.path(), &["config", "show"]);
    assert_eq!(
        config["data"]["protect_defaults"]["runtime_mode"],
        "android_api19"
    );
    assert_eq!(
        config["data"]["protect_defaults"]["environment_policy"],
        "strict"
    );
}

#[test]
#[ignore = "需要真实 JDK；仅在本次隔离目录新建和导入合成证书"]
fn 真实中文证书创建导入重开与校验双向可见() {
    let temp = tempfile::Builder::new()
        .prefix("资料命令 中文空格")
        .tempdir()
        .unwrap();
    for kind in ["jks", "pkcs12"] {
        let output = command(temp.path())
            .env("MOCIKA_SHIELD_KS_PASS", SECRET)
            .args([
                "cert",
                "create",
                "--name",
                "测试证书",
                "--file-name",
                kind,
                "--alias",
                "中文Release",
                "--dname",
                "CN=隔离回归,O=测试",
                "--keystore-type",
                kind,
                "--set-default",
            ])
            .output()
            .unwrap();
        let created = response(output, 0)["data"]["certificate"].clone();
        let id = created["id"].as_str().unwrap();
        let record = store(temp.path()).get_certificate(id).unwrap().unwrap();
        assert_eq!(record.keystore_password, SECRET);
        assert_eq!(record.key_alias, created["key_alias"].as_str().unwrap());
        assert!(record.is_default);
        let value = run(temp.path(), &["cert", "verify", id]);
        assert_eq!(
            value["data"]["certificate"]["last_verify_status"],
            "success"
        );
        let file = created["keystore_path"].as_str().unwrap();
        let bytes = fs::read(file).unwrap();
        let output = command(temp.path())
            .env("MOCIKA_SHIELD_KS_PASS", SECRET)
            .args([
                "cert",
                "import",
                "--name",
                "同一证书外部导入",
                "--keystore",
                file,
                "--alias",
                "中文Release",
                "--keystore-type",
                kind,
            ])
            .output()
            .unwrap();
        let imported = response(output, 0)["data"]["certificate"].clone();
        assert_ne!(imported["id"], created["id"]);
        assert_eq!(fs::read(file).unwrap(), bytes);
        let output = command(temp.path())
            .env("MOCIKA_SHIELD_KS_PASS", "wrong-test-password")
            .args([
                "cert",
                "import",
                "--name",
                "错误密码",
                "--keystore",
                file,
                "--keystore-type",
                kind,
            ])
            .output()
            .unwrap();
        let value = response(output, 1);
        assert!(!value.to_string().contains("wrong-test-password"));
        assert_eq!(
            store(temp.path()).list_certificates().unwrap().len(),
            if kind == "jks" { 2 } else { 4 }
        );
    }
}

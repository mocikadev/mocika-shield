//! 用户名变量有无的真实进程交叉读取；只有合成口令和临时库，不更改测试进程环境。
#![cfg(unix)]
use shield_services::{
    certificates::{CertificateSelection, CertificateStoreState, CertificateUpsertInput},
    paths::ApplicationPaths,
};
use std::{fs, path::Path, process::Stdio};

fn draft() -> CertificateUpsertInput {
    CertificateUpsertInput {
        id: None,
        name: "隔离环境验证".into(),
        source_type: "external".into(),
        keystore_path: String::new(),
        keystore_password: "synthetic-store-password".into(),
        key_alias: "fixture".into(),
        key_password: "synthetic-key-password".into(),
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
    }
}
#[test]
#[ignore = "仅供本文件启动的私有进程夹具，不接触真实库或 Java"]
fn 用户名上下文子进程夹具() {
    let root = std::path::PathBuf::from(std::env::var_os("SHIELD_CONTEXT_TEST_ROOT").unwrap());
    assert!(root
        .file_name()
        .unwrap()
        .to_string_lossy()
        .starts_with("mocika-context-"));
    let paths = ApplicationPaths::isolated(&root).unwrap();
    let mode = std::env::var("SHIELD_CONTEXT_TEST_MODE").unwrap();
    if mode == "create" {
        let store =
            CertificateStoreState::initialize(paths.certificate_database(), paths.keystore_dir())
                .unwrap();
        store
            .save_certificate(&draft(), "fixture-not-a-keystore", "fixture", None)
            .unwrap();
    } else {
        let store = CertificateStoreState::open_read_only(
            paths.certificate_database(),
            paths.keystore_dir(),
        )
        .unwrap();
        let id = store.list_certificates().unwrap().remove(0).id;
        let result = store.get_certificate(&id);
        if mode == "reject" {
            assert!(result.is_err(), "不能猜测其他账户上下文");
            return;
        }
        let value = result.unwrap().unwrap();
        assert!(value.keystore_password == "synthetic-store-password");
        assert!(value.key_password == "synthetic-key-password");
        let bound = store
            .capture_signing_identity(CertificateSelection::Default)
            .ok()
            .unwrap();
        let encoded = serde_json::to_vec(bound.binding()).unwrap();
        assert!(!String::from_utf8_lossy(&encoded).contains("synthetic"));
        let binding = root.join("binding.json");
        if binding.exists() {
            assert_eq!(fs::read(binding).unwrap(), encoded);
        } else {
            fs::write(binding, encoded).unwrap();
        }
    }
}
fn run(root: &Path, mode: &str, user: Option<&str>, username: Option<&str>) {
    let mut command = shield_core::utils::no_window_command(std::env::current_exe().unwrap());
    command
        .args(["--ignored", "--exact", "用户名上下文子进程夹具"])
        .env("SHIELD_CONTEXT_TEST_ROOT", root)
        .env("SHIELD_CONTEXT_TEST_MODE", mode)
        .env_remove("USER")
        .env_remove("USERNAME")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    if let Some(user) = user {
        command.env("USER", user);
    }
    if let Some(username) = username {
        command.env("USERNAME", username);
    }
    assert!(
        command.status().unwrap().success(),
        "隔离环境子进程验证失败"
    );
}
#[test]
fn 真实有无用户名双向读取保持旧密文和所选绑定不变() {
    // 测试端取账户名；生产代码不能依赖该外部程序或 PATH。
    let user = shield_core::utils::no_window_command("/usr/bin/id")
        .arg("-un")
        .output()
        .unwrap();
    assert!(user.status.success());
    let user = String::from_utf8(user.stdout).unwrap();
    let user = user.trim();
    assert!(!user.is_empty());
    for creator in [Some(user), None] {
        let root = tempfile::Builder::new()
            .prefix("mocika-context-")
            .tempdir()
            .unwrap();
        run(root.path(), "create", creator, None);
        let paths = ApplicationPaths::isolated(root.path()).unwrap();
        let before = fs::read(paths.certificate_database()).unwrap();
        for (user, username) in [(Some(user), None), (None, None), (None, Some(user))] {
            run(root.path(), "read", user, username);
            assert_eq!(
                fs::read(paths.certificate_database()).unwrap(),
                before,
                "读取不得重写历史密文"
            );
        }
        if creator.is_some() {
            run(root.path(), "reject", Some("another-synthetic-user"), None);
        }
    }
}

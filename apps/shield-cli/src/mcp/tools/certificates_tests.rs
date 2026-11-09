use super::*;
use std::{fs, os::unix::fs::symlink};
fn fixture(root: &std::path::Path) -> Policy {
    fs::create_dir(root.join("input")).unwrap();
    fs::create_dir(root.join("output")).unwrap();
    fs::write(root.join("input/中文.p12"), b"fixture").unwrap();
    fs::write(root.join("shield"), b"fixture").unwrap();
    Policy::new(
        &root.join("input"),
        &root.join("output"),
        None,
        &root.join("shield"),
        10,
    )
    .unwrap()
    .with_shared(Some(&root.join("home")), vec!["--help".into()])
    .unwrap()
}
#[test]
fn 生命周期只映射固定命令且密码和未知参数无法进入() {
    let root = tempfile::tempdir().unwrap();
    let policy = fixture(root.path());
    let create = json!({"name":"--help","file_name":"发布.p12","alias":"中文Alias","dname":"CN=隔离测试","keystore_type":"pkcs12"});
    let import = json!({"name":"导入测试","keystore":"中文.p12"});
    for (kind, args) in [
        (Management::Create, create.clone()),
        (Management::Import, import.clone()),
    ] {
        let invocation = call(kind, args.clone(), &policy).unwrap();
        assert!(invocation.host_credentials);
        assert!(!invocation.controlled);
        for key in [
            "password",
            "password_env",
            "keystore_password",
            "config",
            "app_home",
            "yes",
            "sql",
        ] {
            let mut bad = args.clone();
            bad[key] = json!("不得接收");
            assert!(call(kind, bad, &policy).is_err());
        }
    }
    let invocation = call(Management::Create, create.clone(), &policy).unwrap();
    assert!(invocation.arguments.contains(&"--name=--help".into()));
    for kind in [Management::Verify, Management::Remove] {
        let invocation = call(kind, json!({"certificate_id":"--help"}), &policy).unwrap();
        assert!(!invocation.host_credentials);
        assert_eq!(
            &invocation.arguments[invocation.arguments.len() - 2..],
            &[OsString::from("--"), OsString::from("--help")]
        );
        assert_eq!(
            invocation.arguments.contains(&"--yes".into()),
            matches!(kind, Management::Remove)
        );
        assert!(call(kind, json!({"certificate_id":"other"}), &policy).is_err());
    }
    for (key, value) in [
        ("file_name", json!("../x.p12")),
        ("alias", json!(null)),
        ("key_size", json!(1024)),
        ("validity_days", json!(0)),
        ("keystore", json!("中文.p12")),
        ("name", json!("x".repeat(257))),
    ] {
        let mut bad = create.clone();
        bad[key] = value;
        assert!(call(Management::Create, bad, &policy).is_err());
    }
    symlink(
        root.path().join("input/中文.p12"),
        root.path().join("input/link.p12"),
    )
    .unwrap();
    fs::write(root.path().join("input/尾空白.p12 "), b"fixture").unwrap();
    for source in ["../shield", "/tmp/x.p12", "link.p12", "尾空白.p12 ", ""] {
        let mut bad = import.clone();
        bad["keystore"] = json!(source);
        assert!(call(Management::Import, bad, &policy).is_err());
    }
    assert!(!root.path().join("home").exists());
}
#[test]
fn 本会话新增及撤销范围有界而导入材料改变可检测() {
    let root = tempfile::tempdir().unwrap();
    let policy = fixture(root.path());
    let source = policy.import_source("中文.p12").unwrap();
    assert!(source.check(&policy).is_ok());
    fs::write(root.path().join("input/中文.p12"), b"changed").unwrap();
    assert!(source.check(&policy).is_err());
    for index in 0..63 {
        policy
            .add_created_certificate(&format!("new-{index}"))
            .unwrap();
    }
    assert!(policy.can_add_certificate().is_err());
    assert!(policy.certificate("new-0").is_ok());
    assert!(policy.certificate("other").is_err());
    policy.remove_certificate("new-0").unwrap();
    assert!(policy.certificate("new-0").is_err());
    assert!(policy.can_add_certificate().is_ok());
    assert!(policy.add_created_certificate("--help").is_err());
}

use super::*;
fn policy(root: &std::path::Path) -> Policy {
    for name in ["input", "output"] {
        std::fs::create_dir(root.join(name)).unwrap();
    }
    std::fs::write(root.join("shield"), b"fixture").unwrap();
    std::fs::write(root.join("project.toml"), b"fixture").unwrap();
    Policy::new(
        &root.join("input"),
        &root.join("output"),
        Some(&root.join("project.toml")),
        &root.join("shield"),
        10,
    )
    .unwrap()
    .with_shared(
        Some(&root.join("home")),
        vec!["--help".into(), "allowed".into()],
    )
    .unwrap()
}
#[test]
fn 设置区分省略清除和保留并且不继承项目覆盖() {
    let root = tempfile::tempdir().unwrap();
    let policy = policy(root.path());
    let (call, _) = management_call(
        "shield_update_settings",
        json!({"runtime_mode":"android_api19","certificate_id":null,"sign_after_protect":false}),
        &policy,
    )
    .unwrap();
    for flag in [
        "--runtime-mode=android_api19",
        "--clear-certificate",
        "--sign-after-protect=false",
    ] {
        assert!(call.arguments.contains(&flag.into()));
    }
    for forbidden in [
        "--config",
        "--inherit-auto-sign",
        "--environment-policy=compatible",
    ] {
        assert!(!call.arguments.contains(&forbidden.into()));
    }
    assert!(!root.path().join("home").exists());
    for (args, code) in [
        (json!({}), "PATCH_REQUIRED"),
        (
            json!({"runtime_mode":null,"environment_policy":"strict"}),
            "ARGUMENT_INVALID",
        ),
        (
            json!({"certificate_id":"hidden"}),
            "CERTIFICATE_NOT_ALLOWED",
        ),
        (
            json!({"fixed_output_directory":"/somewhere"}),
            "ARGUMENT_INVALID",
        ),
    ] {
        assert_eq!(
            management_call("shield_update_settings", args, &policy)
                .err()
                .unwrap(),
            code
        );
    }
}
#[test]
fn 文本和短横线编号只能作为数据且禁止材料参数() {
    let root = tempfile::tempdir().unwrap();
    let policy = policy(root.path());
    let (call, _) = management_call(
        "shield_update_certificate",
        json!({"certificate_id":"--help","name":"--help","note":"中文; $(不执行)","v3":false}),
        &policy,
    )
    .unwrap();
    assert!(call.arguments.contains(&"--name=--help".into()));
    assert_eq!(
        &call.arguments[call.arguments.len() - 2..],
        &[OsString::from("--"), OsString::from("--help")]
    );
    for field in [
        "password",
        "key_alias",
        "keystore",
        "ks_type",
        "database",
        "config",
    ] {
        let mut args = json!({"certificate_id":"allowed","name":"显示名称"});
        args[field] = json!("private-value");
        assert_eq!(
            management_call("shield_update_certificate", args, &policy)
                .err()
                .unwrap(),
            "ARGUMENT_INVALID"
        );
    }
    for value in [json!(null), json!("\u{0000}"), json!("x".repeat(257))] {
        assert!(management_call(
            "shield_update_certificate",
            json!({"certificate_id":"allowed","name":value}),
            &policy
        )
        .is_err());
    }
    assert!(management_call(
        "shield_set_default_certificate",
        json!({"certificate_id":null}),
        &policy
    )
    .is_err());
}

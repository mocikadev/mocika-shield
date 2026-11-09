use super::*;
use shield_app::{
    certificates::{CertificateStoreState, CertificateUpsertInput},
    paths::ApplicationPaths,
};

#[test]
fn 共享证书偏好低于项目和本次参数且秘密不从其他来源拼接() {
    let temp = tempfile::tempdir().unwrap();
    let paths = ApplicationPaths::isolated(temp.path()).unwrap();
    let store =
        CertificateStoreState::initialize(paths.certificate_database(), paths.keystore_dir())
            .unwrap();
    let record = store
        .save_certificate(
            &CertificateUpsertInput {
                id: None,
                name: "测试".into(),
                source_type: "external".into(),
                keystore_path: "/fixture.p12".into(),
                keystore_password: "synthetic-password".into(),
                key_alias: "release".into(),
                key_password: "synthetic-key-password".into(),
                ks_type: Some("PKCS12".into()),
                sign_v1: true,
                sign_v2: true,
                sign_v3: false,
                sign_v4: false,
                auto_sign_enabled: false,
                note: String::new(),
                set_as_default: false,
                copy_keystore_to_managed: false,
                managed_file_name: None,
            },
            "/fixture.p12",
            "release",
            None,
        )
        .unwrap();
    let project: CliConfig = toml::from_str(
        "schema_version=1\n[sign]\ninput='input.apk'\noutput='output.apk'\nv2=false\nv3=true\n",
    )
    .unwrap();
    let args = SignArgs {
        v1: Some(false),
        v2: Some(true),
        ..Default::default()
    };
    let resolved = project.merge_shared_sign(args, record.clone()).unwrap();
    assert!(!resolved.v1);
    assert!(resolved.v2 && resolved.v3);
    assert!(!resolved.v4);
    assert_eq!(resolved.keystore_password, "synthetic-password");
    assert_eq!(resolved.key_password, "synthetic-key-password");
    for args in [
        SignArgs {
            ks: Some("another.p12".into()),
            ..Default::default()
        },
        SignArgs {
            ks_pass: Some("not-mixed".into()),
            ..Default::default()
        },
        SignArgs {
            key_alias: Some("another".into()),
            ..Default::default()
        },
    ] {
        assert!(project.require_shared_sign_source(&args).is_err());
    }
    let mixed: CliConfig =
        toml::from_str("schema_version=1\n[sign]\nkeystore='another.p12'\n").unwrap();
    assert!(mixed
        .merge_shared_sign(SignArgs::default(), record)
        .is_err());
}

//! 自有真实加固输入只允许原绑定证书，普通换证书警告不能绕过此规则。
use super::*;
use std::sync::{atomic::AtomicBool, Arc};

#[test]
#[ignore = "额外需要 SHIELD_TEST_RESOURCES 和 SHIELD_TEST_APKTOOL，生成并验证真实加固包"]
fn 真实受控签名保留加固载荷并拒绝错证书和损坏格式() {
    let input = PathBuf::from(std::env::var_os("SHIELD_TEST_APK").expect("指定自有 APK"));
    let signer = PathBuf::from(std::env::var_os("SHIELD_TEST_APKSIGNER").expect("指定签名工具"));
    let resources = PathBuf::from(std::env::var_os("SHIELD_TEST_RESOURCES").expect("指定资源"));
    let apktool = PathBuf::from(std::env::var_os("SHIELD_TEST_APKTOOL").expect("指定解包工具"));
    let temp = tempfile::tempdir().unwrap();
    let paths = ApplicationPaths::isolated(temp.path()).unwrap();
    let store =
        CertificateStoreState::initialize(paths.certificate_database(), paths.keystore_dir())
            .unwrap();
    let certificate = |name: &str| {
        create_managed_certificate(
            &store,
            CreateManagedCertificateInput {
                name: name.into(),
                file_name: name.into(),
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
                dname: "CN=隔离测试".into(),
                validity_days: 7,
                key_size: 2048,
            },
        )
        .unwrap()
    };
    let original = certificate("原证书");
    let other = certificate("不同证书");
    let config = temp.path().join("shield-cli.toml");
    fs::write(
        &config,
        format!(
            "schema_version=1\n[sign]\napksigner={}\n",
            serde_json::to_string(signer.to_str().unwrap()).unwrap()
        ),
    )
    .unwrap();
    let signed_input = temp.path().join("原签名.apk");
    result(
        command(temp.path(), Some(&config), "sign", &input, &signed_input)
            .arg("--certificate-id")
            .arg(&original.id)
            .output()
            .unwrap(),
        true,
    );
    let expected = shield_core::extract_apk_cert_fingerprint(&signed_input, Some(&signer)).unwrap();
    let protected = temp.path().join("加固未签名.apk");
    shield_core::protect_apk(
        &shield_core::ProtectOptions {
            input: signed_input,
            output: protected.clone(),
            excluded_abis: vec![],
            apktool_path: Some(apktool),
            resources_path: Some(resources),
            apksigner_path: Some(signer.clone()),
            expected_output_cert_fingerprint: Some(expected.clone()),
            environment_policy: shield_core::EnvironmentPolicy::Compatible,
        },
        |_| {},
        Arc::new(AtomicBool::new(false)),
    )
    .unwrap();
    let before = fs::read(&protected).unwrap();
    let content = shield_core::inspect_signing_input(&protected, || Ok(())).unwrap();
    assert!(content.certificate_binding.is_some());
    let wrong = temp.path().join("不应产生.apk");
    let value = result(
        command(temp.path(), Some(&config), "sign", &protected, &wrong)
            .arg("--certificate-id")
            .arg(&other.id)
            .output()
            .unwrap(),
        false,
    );
    assert_eq!(value["code"], "CERTIFICATE_MISMATCH");
    assert!(!wrong.exists());
    let out = temp.path().join("加固已签名.apk");
    result(
        command(temp.path(), Some(&config), "sign", &protected, &out)
            .arg("--certificate-id")
            .arg(&original.id)
            .output()
            .unwrap(),
        true,
    );
    assert_eq!(
        shield_core::inspect_signing_input(&out, || Ok(())).unwrap(),
        content
    );
    assert_eq!(
        shield_core::extract_apk_cert_fingerprint(&out, Some(&signer)).unwrap(),
        expected
    );
    assert!(
        shield_core::utils::java_command(shield_core::utils::find_java().unwrap())
            .arg("-jar")
            .arg(&signer)
            .arg("verify")
            .arg(&out)
            .output()
            .unwrap()
            .status
            .success()
    );
    let again = temp.path().join("加固同证书重签.apk");
    result(
        command(temp.path(), Some(&config), "sign", &out, &again)
            .arg("--certificate-id")
            .arg(&original.id)
            .output()
            .unwrap(),
        true,
    );
    assert_eq!(
        shield_core::inspect_signing_input(&again, || Ok(())).unwrap(),
        content
    );
    let broken = temp.path().join("无效输入.apk");
    fs::write(&broken, b"not an APK").unwrap();
    let value = result(
        command(temp.path(), Some(&config), "sign", &broken, &wrong)
            .arg("--certificate-id")
            .arg(&original.id)
            .output()
            .unwrap(),
        false,
    );
    assert_eq!(value["code"], "SIGNING_INPUT_INVALID");
    let wrong_password = store
        .save_certificate(
            &CertificateUpsertInput {
                id: None,
                name: "错误密码测试".into(),
                source_type: "external".into(),
                keystore_path: String::new(),
                keystore_password: "incorrect-test-password".into(),
                key_alias: original.key_alias.clone(),
                key_password: String::new(),
                ks_type: Some("PKCS12".into()),
                sign_v1: true,
                sign_v2: true,
                sign_v3: true,
                sign_v4: false,
                auto_sign_enabled: false,
                note: String::new(),
                set_as_default: false,
                copy_keystore_to_managed: false,
                managed_file_name: None,
            },
            &original.keystore_path,
            &original.key_alias,
            None,
        )
        .unwrap();
    let value = result(
        command(temp.path(), Some(&config), "sign", &input, &wrong)
            .arg("--certificate-id")
            .arg(wrong_password.id)
            .output()
            .unwrap(),
        false,
    );
    assert_eq!(value["output_state"], "not_published");
    assert!(!value.to_string().contains("incorrect-test-password"));
    assert_eq!(fs::read(&protected).unwrap(), before);
    assert!(!wrong.exists());
}

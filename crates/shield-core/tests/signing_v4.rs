//! 真实签名参数与内容不变量回归，显式运行，不增加逐提交的重型测试。
use shield_core::utils::{find_java, find_keytool, java_command, no_window_command};
use shield_core::{
    extract_apk_cert_fingerprint, extract_keystore_cert_fingerprint, inspect_signing_input,
    sign_apk, KeystoreType, SignOptions, SigningVersions,
};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::{atomic::AtomicBool, Arc},
};

const PASSWORD: &str = "synthetic-v4-test-only-password";

fn run(mut command: Command, stage: &str) {
    let result = command.output().expect("无法启动测试工具");
    // 不输出 Command、环境或原始工具日志，避免未来修改夹具时泄露材料。
    assert!(result.status.success(), "{stage}失败");
}

fn certificate(root: &Path, ks_type: &KeystoreType) -> (PathBuf, String) {
    let keystore = root.join("中文测试证书.p12");
    let mut command = no_window_command(find_keytool().unwrap());
    command
        .args([
            "-genkeypair",
            "-alias",
            "签名验证",
            "-keyalg",
            "RSA",
            "-keysize",
            "2048",
            "-validity",
            "2",
            "-dname",
            "CN=本地签名验证,O=临时测试",
            "-storetype",
            ks_type.as_str(),
            "-keystore",
        ])
        .arg(&keystore)
        .args([
            "-storepass:env",
            "MOCIKA_V4_TEST_PASS",
            "-keypass:env",
            "MOCIKA_V4_TEST_PASS",
            "-noprompt",
        ])
        .env("MOCIKA_V4_TEST_PASS", PASSWORD);
    run(command, "创建临时证书");
    let expected =
        extract_keystore_cert_fingerprint(&keystore, "签名验证", PASSWORD, Some(ks_type.as_str()))
            .unwrap();
    (keystore, expected)
}

fn sign(
    input: &Path,
    output: &Path,
    keystore: &Path,
    signer: &Path,
    ks_type: KeystoreType,
    v4: bool,
) {
    sign_apk(&SignOptions {
        apk_path: input.to_owned(),
        output_path: Some(output.to_owned()),
        keystore_path: keystore.to_owned(),
        key_alias: "签名验证".into(),
        keystore_password: PASSWORD.into(),
        key_password: PASSWORD.into(),
        apksigner_path: Some(signer.to_owned()),
        keystore_type: ks_type,
        signing_versions: SigningVersions {
            v4,
            ..SigningVersions::default()
        },
    })
    .expect("真实签名失败");
}

#[test]
#[ignore = "需要真实 JDK、自有 SHIELD_TEST_APK、SHIELD_TEST_APKSIGNER 及 SHIELD_TEST_ZIPALIGN"]
fn 真实签名显式关闭和开启v4且保留业务内容() {
    let input = PathBuf::from(std::env::var_os("SHIELD_TEST_APK").expect("指定自有 APK"));
    let signer = PathBuf::from(std::env::var_os("SHIELD_TEST_APKSIGNER").expect("指定 apksigner"));
    let zipalign =
        PathBuf::from(std::env::var_os("SHIELD_TEST_ZIPALIGN").expect("指定 SDK zipalign"));
    let before = fs::read(&input).unwrap();
    let content = inspect_signing_input(&input, || Ok(())).unwrap();
    let temp = tempfile::tempdir().unwrap();
    for ks_type in [KeystoreType::Jks, KeystoreType::Pkcs12] {
        let root = temp.path().join(ks_type.as_str());
        fs::create_dir(&root).unwrap();
        let (keystore, expected) = certificate(&root, &ks_type);
        for v4 in [false, true] {
            let output = root.join(format!("中文签名-{v4}.apk"));
            sign(&input, &output, &keystore, &signer, ks_type.clone(), v4);
            assert_eq!(
                output
                    .with_file_name(format!("中文签名-{v4}.apk.idsig"))
                    .exists(),
                v4
            );
            let mut verify = java_command(find_java().unwrap());
            verify.arg("-jar").arg(&signer).arg("verify").arg(&output);
            run(verify, "独立验签");
            let mut align = no_window_command(&zipalign);
            align.args(["-c", "-P", "16", "4"]).arg(&output);
            run(align, "SDK 对齐检查");
            assert_eq!(
                extract_apk_cert_fingerprint(&output, Some(&signer)).unwrap(),
                expected
            );
            assert_eq!(inspect_signing_input(&output, || Ok(())).unwrap(), content);
        }
    }
    assert_eq!(fs::read(&input).unwrap(), before);
}

#[test]
#[ignore = "额外需要 SHIELD_TEST_RESOURCES 和 SHIELD_TEST_APKTOOL，生成自有真实加固包"]
fn 真实加固包读取绑定且独立签名保留载荷() {
    let input = PathBuf::from(std::env::var_os("SHIELD_TEST_APK").expect("指定自有 APK"));
    let signer = PathBuf::from(std::env::var_os("SHIELD_TEST_APKSIGNER").expect("指定 apksigner"));
    let resources = PathBuf::from(std::env::var_os("SHIELD_TEST_RESOURCES").expect("指定当前资源"));
    let apktool = PathBuf::from(std::env::var_os("SHIELD_TEST_APKTOOL").expect("指定 apktool"));
    let zipalign =
        PathBuf::from(std::env::var_os("SHIELD_TEST_ZIPALIGN").expect("指定 SDK zipalign"));
    let temp = tempfile::tempdir().unwrap();
    let (keystore, expected) = certificate(temp.path(), &KeystoreType::Pkcs12);
    let signed_input = temp.path().join("原始签名.apk");
    sign(
        &input,
        &signed_input,
        &keystore,
        &signer,
        KeystoreType::Pkcs12,
        false,
    );
    let before = fs::read(&signed_input).unwrap();
    let protected = temp.path().join("加固未签名.apk");
    shield_core::protect_apk(
        &shield_core::ProtectOptions {
            excluded_abis: Vec::new(),
            input: signed_input.clone(),
            output: protected.clone(),
            apktool_path: Some(apktool),
            resources_path: Some(resources),
            apksigner_path: Some(signer.clone()),
            expected_output_cert_fingerprint: Some(expected.clone()),
            environment_policy: shield_core::EnvironmentPolicy::Compatible,
        },
        |_| {},
        Arc::new(AtomicBool::new(false)),
    )
    .expect("自有加固包生成失败");
    let declared = inspect_signing_input(&protected, || Ok(())).expect("读取真实绑定失败");
    let mut digest = [0u8; 32];
    for (index, pair) in shield_core::normalize_fingerprint(&expected)
        .as_bytes()
        .as_chunks::<2>()
        .0
        .iter()
        .enumerate()
    {
        digest[index] = u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap();
    }
    assert_eq!(declared.certificate_binding, Some(digest));
    let signed = temp.path().join("加固已签名.apk");
    sign(
        &protected,
        &signed,
        &keystore,
        &signer,
        KeystoreType::Pkcs12,
        false,
    );
    assert_eq!(inspect_signing_input(&signed, || Ok(())).unwrap(), declared);
    assert_eq!(
        extract_apk_cert_fingerprint(&signed, Some(&signer)).unwrap(),
        expected
    );
    let mut verify = java_command(find_java().unwrap());
    verify.arg("-jar").arg(&signer).arg("verify").arg(&signed);
    run(verify, "加固包独立验签");
    let mut align = no_window_command(zipalign);
    align.args(["-c", "-P", "16", "4"]).arg(&signed);
    run(align, "加固包 SDK 对齐检查");
    assert_eq!(fs::read(&signed_input).unwrap(), before);
}

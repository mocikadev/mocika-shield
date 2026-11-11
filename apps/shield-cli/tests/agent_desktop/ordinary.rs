//! 真实普通 CLI 默认链路，不以受控 Agent／MCP 成功替代。
use super::*;

fn verify(source: &Path, fixture: &Fixture, fingerprint: &str) {
    let reference = tempfile::tempdir().unwrap();
    assert!(reference.path().as_os_str().is_ascii());
    let copy = reference.path().join("verify.apk");
    fs::copy(source, &copy).unwrap();
    assert!(fs::read(source).unwrap() == fs::read(&copy).unwrap());
    let result = shield_core::utils::java_command(&fixture.java)
        .arg("-jar")
        .arg(std::env::var_os("SHIELD_TEST_APKSIGNER").unwrap())
        .args(["verify", "--verbose", "--print-certs"])
        .arg(&copy)
        .output()
        .unwrap();
    assert!(result.status.success(), "独立原工具验签失败");
    assert!(String::from_utf8(result.stdout)
        .unwrap()
        .to_ascii_uppercase()
        .contains(fingerprint));
    assert!(shield_core::utils::no_window_command(
        std::env::var_os("SHIELD_TEST_ZIPALIGN").unwrap()
    )
    .args(["-c", "-P", "16", "4"])
    .arg(&copy)
    .output()
    .unwrap()
    .status
    .success());
}

#[test]
#[ignore = "需真实自有 DEX APK、壳资源和 Java／SDK；实际调用普通 sign／protect 命令"]
fn 真实普通共享签名加固再签名保持资料不变() {
    let fixture = Fixture::new();
    let parent = fixture.root.path();
    let paths = ApplicationPaths::isolated(&parent.join("home")).unwrap();
    let store =
        CertificateStoreState::initialize(paths.certificate_database(), paths.keystore_dir())
            .unwrap();
    let record = store
        .get_certificate(&fixture.certificate)
        .unwrap()
        .unwrap();
    let database = fs::read(paths.certificate_database()).unwrap();
    let certificate = fs::read(&record.keystore_path).unwrap();
    let fingerprint = shield_core::extract_keystore_cert_fingerprint(
        Path::new(&record.keystore_path),
        &record.key_alias,
        &record.keystore_password,
        Some(&record.ks_type),
    )
    .unwrap();
    let seed = parent.join("input/普通中文输入😀.apk");
    let output = parent.join("output/普通中文加固😀.apk");
    let mut sign = fixture.command(false);
    sign.args(["sign", "--use-default-certificate", "--input"])
        .arg(parent.join("input/app.apk"))
        .arg("--output")
        .arg(&seed)
        .arg("--json");
    let result = sign.output().unwrap();
    assert!(result.status.success(), "普通共享签名失败");
    assert!(!String::from_utf8_lossy(&result.stdout).contains(SECRET));
    verify(&seed, &fixture, &fingerprint);
    let signed = fs::read(&seed).unwrap();
    let mut protect = fixture.command(false);
    protect
        .args([
            "protect",
            "--runtime-profile",
            "standard",
            "--use-default-certificate",
            "--input",
        ])
        .arg(&seed)
        .arg("--output")
        .arg(&output)
        .arg("--json");
    let result = protect.output().unwrap();
    assert!(result.status.success(), "普通共享加固及签名失败");
    assert!(!String::from_utf8_lossy(&result.stdout).contains(SECRET));
    verify(&output, &fixture, &fingerprint);
    let binding = shield_core::inspect_signing_input(&output, || Ok(()))
        .unwrap()
        .certificate_binding
        .unwrap();
    assert_eq!(
        binding
            .iter()
            .map(|byte| format!("{byte:02X}"))
            .collect::<String>(),
        fingerprint
    );
    assert!(fs::read(&seed).unwrap() == signed);
    assert!(fs::read(&record.keystore_path).unwrap() == certificate);
    assert!(fs::read(paths.certificate_database()).unwrap() == database);
    fixture.check_unchanged();
    println!("普通 CLI 完整链路：共用默认证书签名、标准加固再签名、中文及补充字符业务路径、独立原工具验签和 SDK 对齐、证书绑定、输入与证书库不变均通过");
}

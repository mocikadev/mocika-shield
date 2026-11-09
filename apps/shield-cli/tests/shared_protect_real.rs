//! 显式运行的本机真实闭环；只使用自有 APK、临时证书及隔离应用目录。
use serde_json::{json, Value};
use shield_core::{
    check_apk, extract_apk_cert_fingerprint, extract_keystore_cert_fingerprint,
    normalize_fingerprint, KeystoreType, SignOptions, SigningVersions,
};
use shield_services::{
    certificates::{
        create_managed_certificate, save_certificate_profile, CertificateStoreState,
        CertificateUpsertInput, CreateManagedCertificateInput,
    },
    config::ConfigStore,
    paths::ApplicationPaths,
};
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
};

const PASSWORD: &str = "temporary-shared-protect-password";
fn command(root: &Path, input: &Path, output: &Path, signer: &Path, apktool: &Path) -> Command {
    let bundle = std::env::var_os("SHIELD_SHARED_TEST_APP").map(PathBuf::from);
    let program = bundle
        .as_ref()
        .map(|app| {
            assert!(
                app.is_absolute() && app.is_dir(),
                "应用包路径无效，不回退源码二进制"
            );
            app.join("Contents/MacOS/shield")
        })
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_BIN_EXE_shield")));
    let mut command = Command::new(program);
    // 源码二进制的开发资源回退依赖仓库工作目录；完整应用包必须在源码外自行定位。
    let working_dir = if bundle.is_some() {
        root.to_path_buf()
    } else {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()
            .unwrap()
    };
    command
        .current_dir(working_dir)
        .arg("--app-home")
        .arg(root)
        .args(["protect", "--json", "-i"])
        .arg(input)
        .arg("-o")
        .arg(output)
        .env_remove("MOCIKA_SHIELD_KS_PASS")
        .env_remove("MOCIKA_SHIELD_KEY_PASS");
    if bundle.is_none() {
        command
            .arg("--apksigner")
            .arg(signer)
            .arg("--apktool")
            .arg(apktool);
    }
    command
}
fn events(output: Output, success: bool) -> Vec<Value> {
    assert!(output.stderr.is_empty());
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(!text.contains(PASSWORD));
    assert!(!text.contains("wrong-key-password"));
    let events: Vec<Value> = text
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(
        output.status.success(),
        success,
        "真实命令退出状态不符合预期；脱敏终态：{}",
        events
            .last()
            .and_then(|value| value["message"].as_str())
            .unwrap_or("无错误说明")
    );
    assert_eq!(
        events.last().unwrap()["type"],
        if success { "done" } else { "error" }
    );
    assert_eq!(
        events
            .iter()
            .filter(|event| event["type"] == "done")
            .count(),
        usize::from(success)
    );
    events
}

#[test]
#[ignore = "需要真实 JDK、自有 SHIELD_TEST_APK 及 SHIELD_TEST_ZIPALIGN，运行完整加固流程"]
fn 真实共享加固双模式外部配置及签名失败不交付() {
    let original = PathBuf::from(std::env::var_os("SHIELD_TEST_APK").expect("指定自有测试 APK"));
    let zipalign =
        PathBuf::from(std::env::var_os("SHIELD_TEST_ZIPALIGN").expect("指定 SDK 对齐工具"));
    let repo = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap();
    let signer = repo.join("tools/apksigner.jar");
    let apktool = repo.join("tools/apktool_3.0.1.jar");
    let original_bytes = fs::read(&original).unwrap();
    use sha2::Digest;
    let bundle_files = std::env::var_os("SHIELD_SHARED_TEST_APP")
        .map(PathBuf::from)
        .map(|app| {
            [
                "Contents/MacOS/shield",
                "Contents/MacOS/mocika-shield",
                "Contents/Resources/tools/apksigner.jar",
                "Contents/Resources/tools/apktool.jar",
                "Contents/Resources/resources/resources.zip",
                "Contents/Resources/resources/resources-api19.zip",
            ]
            .into_iter()
            .map(|name| {
                let path = app.join(name);
                let hash: [u8; 32] = sha2::Sha256::digest(fs::read(&path).unwrap()).into();
                (path, hash)
            })
            .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let temp = tempfile::Builder::new()
        .prefix("共享加固 中文空格")
        .tempdir()
        .unwrap();
    let paths = ApplicationPaths::isolated(temp.path()).unwrap();
    let store =
        CertificateStoreState::initialize(paths.certificate_database(), paths.keystore_dir())
            .unwrap();
    let certificate = create_managed_certificate(
        &store,
        CreateManagedCertificateInput {
            name: "真实共享签名".into(),
            file_name: "fixture.jks".into(),
            key_alias: "中文Release".into(),
            keystore_password: PASSWORD.into(),
            key_password: PASSWORD.into(),
            ks_type: Some("JKS".into()),
            sign_v1: true,
            sign_v2: true,
            sign_v3: true,
            sign_v4: false,
            auto_sign_enabled: true,
            note: "仅用于回归".into(),
            set_as_default: true,
            dname: "CN=共享加固测试,O=临时".into(),
            validity_days: 7,
            key_size: 2048,
        },
    )
    .unwrap();
    let input = temp.path().join("原始签名副本.apk");
    shield_core::sign_apk(&SignOptions {
        apk_path: original.clone(),
        output_path: Some(input.clone()),
        keystore_path: certificate.keystore_path.clone().into(),
        key_alias: certificate.key_alias.clone(),
        keystore_password: PASSWORD.into(),
        key_password: PASSWORD.into(),
        apksigner_path: Some(signer.clone()),
        keystore_type: KeystoreType::Jks,
        signing_versions: SigningVersions::default(),
    })
    .unwrap();
    let input_bytes = fs::read(&input).unwrap();
    let keystore_bytes = fs::read(&certificate.keystore_path).unwrap();
    ConfigStore::new(paths.config_file()).update_protect_defaults(serde_json::from_value(json!({
        "runtime_mode":"android_api19", "sign_after_protect":true, "certificate_id":certificate.id,
    })).unwrap()).unwrap();
    let database_bytes = fs::read(paths.certificate_database()).unwrap();
    let config_bytes = fs::read(paths.config_file()).unwrap();
    let expected = extract_keystore_cert_fingerprint(
        Path::new(&certificate.keystore_path),
        &certificate.key_alias,
        PASSWORD,
        Some("JKS"),
    )
    .unwrap();
    for mode in ["android-api19", "standard"] {
        let output = temp.path().join(format!("输出-{mode}.apk"));
        let mut invocation = command(temp.path(), &input, &output, &signer, &apktool);
        if mode == "standard" {
            invocation.args(["--runtime-profile", "standard"]);
        }
        let progress = events(invocation.output().unwrap(), true);
        assert!(progress.iter().any(|event| event["step"] == "sign"));
        let checked = check_apk(&output, Some(&signer)).unwrap();
        assert!(checked.already_protected && checked.is_signed);
        assert_eq!(
            normalize_fingerprint(&extract_apk_cert_fingerprint(&output, Some(&signer)).unwrap()),
            normalize_fingerprint(&expected)
        );
        // 签名工具会新增压缩的签名条目；安装偏移约束仅作用于 Stored 条目。
        {
            let mut archive = zip::ZipArchive::new(fs::File::open(&output).unwrap()).unwrap();
            for issue in shield_core::verify_apk_alignment(&output).unwrap() {
                assert_ne!(
                    archive.by_name(&issue.name).unwrap().compression(),
                    zip::CompressionMethod::Stored,
                    "不压缩条目未对齐：{}",
                    issue.name
                );
            }
        }
        assert!(Command::new(&zipalign)
            .args(["-c", "-P", "16", "4"])
            .arg(&output)
            .output()
            .unwrap()
            .status
            .success());
        assert_eq!(fs::read(&input).unwrap(), input_bytes);
        assert_eq!(
            fs::read(paths.certificate_database()).unwrap(),
            database_bytes
        );
        assert_eq!(fs::read(paths.config_file()).unwrap(), config_bytes);
    }
    // 显式关闭压过共享默认的自动签名意图，仍可单独交付未签名加固包。
    let unsigned_output = temp.path().join("仅加固输出.apk");
    let progress = events(
        command(temp.path(), &input, &unsigned_output, &signer, &apktool)
            .args([
                "--runtime-profile",
                "standard",
                "--sign-after-protect",
                "false",
            ])
            .output()
            .unwrap(),
        true,
    );
    assert!(!progress.iter().any(|event| event["step"] == "sign"));
    let checked = check_apk(&unsigned_output, Some(&signer)).unwrap();
    assert!(checked.already_protected && !checked.is_signed);
    // 新普通入口也保留项目文件的临时材料；密码仅由本次宿主环境提供。
    let project = temp.path().join("shield-cli.toml");
    fs::write(&project, format!("schema_version=1\n[protect]\nuse_shared_settings=false\nsign_after_protect=true\nruntime_mode='standard'\n[sign]\nkeystore={}\nkey_alias={}\nkeystore_type='jks'\n",
        serde_json::to_string(&certificate.keystore_path).unwrap(), serde_json::to_string(&certificate.key_alias).unwrap())).unwrap();
    let external_output = temp.path().join("外部配置输出.apk");
    events(
        command(temp.path(), &input, &external_output, &signer, &apktool)
            .arg("--config")
            .arg(&project)
            .env("MOCIKA_SHIELD_KS_PASS", PASSWORD)
            .output()
            .unwrap(),
        true,
    );
    assert_eq!(
        normalize_fingerprint(
            &extract_apk_cert_fingerprint(&external_output, Some(&signer)).unwrap()
        ),
        normalize_fingerprint(&expected)
    );
    assert_eq!(
        fs::read(paths.certificate_database()).unwrap(),
        database_bytes
    );

    // 模式与资源冲突在解包前停止，不能悄悄切换资源。
    let rejected = temp.path().join("模式冲突.apk");
    let progress = events(
        command(temp.path(), &input, &rejected, &signer, &apktool)
            .args(["--runtime-profile", "standard", "--resources"])
            .arg(repo.join("shield-stub/build/outputs/resources/resources-api19.zip"))
            .output()
            .unwrap(),
        false,
    );
    assert!(!progress.iter().any(|event| event["step"] == "Unpack"));
    assert!(!rejected.exists());

    let bad = save_certificate_profile(
        &store,
        CertificateUpsertInput {
            id: None,
            name: "错误 Key 密码夹具".into(),
            source_type: "external".into(),
            keystore_path: certificate.keystore_path.clone(),
            keystore_password: PASSWORD.into(),
            key_alias: certificate.key_alias.clone(),
            key_password: "wrong-key-password".into(),
            ks_type: Some("JKS".into()),
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
    )
    .unwrap();
    let failed = temp.path().join("签名失败不应出现.apk");
    let progress = events(
        command(temp.path(), &input, &failed, &signer, &apktool)
            .args(["--runtime-profile", "standard", "--certificate-id", &bad.id])
            .output()
            .unwrap(),
        false,
    );
    assert!(
        progress.iter().any(|event| event["step"] == "sign"),
        "须实际走到签名再验证失败清理"
    );
    assert!(!failed.exists());
    assert!(!fs::read_dir(temp.path()).unwrap().any(|entry| entry
        .unwrap()
        .file_name()
        .to_string_lossy()
        .starts_with(".mocika-protect-")));
    assert_eq!(fs::read(&original).unwrap(), original_bytes);
    assert_eq!(fs::read(&input).unwrap(), input_bytes);
    assert_eq!(
        fs::read(&certificate.keystore_path).unwrap(),
        keystore_bytes
    );
    assert_eq!(fs::read(paths.config_file()).unwrap(), config_bytes);
    for (path, before) in bundle_files {
        let after: [u8; 32] = sha2::Sha256::digest(fs::read(path).unwrap()).into();
        assert_eq!(after, before, "包内文件不得被任务改写");
    }
}

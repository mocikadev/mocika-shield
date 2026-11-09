//! 单次受控独立签名：只使用隔离资料，标准输入关闭也无需密码或确认交互。
#![cfg(all(feature = "agent-executor", target_os = "macos"))]
use serde_json::Value;
use shield_services::{
    certificates::{
        create_managed_certificate, CertificateStoreState, CertificateUpsertInput,
        CreateManagedCertificateInput,
    },
    paths::ApplicationPaths,
};
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
};

const SECRET: &str = "controlled-signing-test-password";
#[path = "agent_signing/control.rs"]
mod control;
#[path = "agent_signing/protected.rs"]
mod protected;
fn command(
    home: &Path,
    config: Option<&Path>,
    operation: &str,
    input: &Path,
    output: &Path,
) -> Command {
    command_timeout(home, config, operation, input, output, 30)
}
fn command_timeout(
    home: &Path,
    config: Option<&Path>,
    operation: &str,
    input: &Path,
    output: &Path,
    timeout: u64,
) -> Command {
    let mut cmd = shield_core::utils::no_window_command(env!("CARGO_BIN_EXE_shield"));
    cmd.arg("--app-home").arg(home);
    if let Some(config) = config {
        cmd.arg("--config").arg(config);
    }
    cmd.args(["agent", operation, "--timeout-seconds"])
        .arg(timeout.to_string())
        .arg("-i")
        .arg(input)
        .arg("-o")
        .arg(output)
        .stdin(Stdio::null())
        .env_remove("MOCIKA_SHIELD_KS_PASS")
        .env_remove("MOCIKA_SHIELD_KEY_PASS")
        .env_remove("JAVA_TOOL_OPTIONS")
        .env_remove("_JAVA_OPTIONS")
        .env_remove("JDK_JAVA_OPTIONS");
    cmd
}
fn result(output: Output, success: bool) -> Value {
    assert_eq!(output.status.success(), success, "单次签名退出状态错误");
    assert!(output.stderr.is_empty());
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(!text.contains(SECRET));
    let values: Vec<Value> = text
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(values.iter().filter(|v| v["type"] == "result").count(), 1);
    for (index, event) in values.iter().enumerate() {
        assert_eq!(event["sequence"], index);
        assert_eq!(event["schema_version"], 1);
        assert!(!event.to_string().contains("keystore_password"));
    }
    values.last().unwrap().clone()
}
fn record(root: &Path, versions: [bool; 4]) -> CertificateStoreState {
    let paths = ApplicationPaths::isolated(root).unwrap();
    let store =
        CertificateStoreState::initialize(paths.certificate_database(), paths.keystore_dir())
            .unwrap();
    let file = root.join("仅门禁测试资料.p12");
    fs::write(&file, "签名版本检查必须先于解析此文件").unwrap();
    store
        .save_certificate(
            &CertificateUpsertInput {
                id: None,
                name: "测试".into(),
                source_type: "external".into(),
                keystore_path: String::new(),
                keystore_password: SECRET.into(),
                key_alias: "中文别名".into(),
                key_password: String::new(),
                ks_type: Some("PKCS12".into()),
                sign_v1: versions[0],
                sign_v2: versions[1],
                sign_v3: versions[2],
                sign_v4: versions[3],
                auto_sign_enabled: false,
                note: String::new(),
                set_as_default: true,
                copy_keystore_to_managed: false,
                managed_file_name: None,
            },
            file.to_str().unwrap(),
            "中文别名",
            None,
        )
        .unwrap();
    store
}

#[test]
fn 无共享库时返回有限错误而不创建库或询问密码() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("未配置");
    let out = temp.path().join("输出.apk");
    let value = result(
        command(&home, None, "sign", Path::new("不存在.apk"), &out)
            .output()
            .unwrap(),
        false,
    );
    assert_eq!(value["code"], "CREDENTIAL_UNAVAILABLE");
    assert_eq!(value["output_state"], "not_published");
    assert!(!home.exists() && !out.exists());
}
#[test]
fn 版本门禁不降级也不为签名要求壳工具() {
    for (versions, code) in [
        ([true, true, true, true], "SIGNING_V4_NOT_SUPPORTED"),
        ([false, false, false, false], "SIGNING_VERSIONS_INVALID"),
    ] {
        let temp = tempfile::tempdir().unwrap();
        let _store = record(temp.path(), versions);
        let out = temp.path().join("输出.apk");
        let value = result(
            command(temp.path(), None, "sign", Path::new("不存在.apk"), &out)
                .output()
                .unwrap(),
            false,
        );
        assert_eq!(value["code"], code);
        assert!(!out.exists());
    }
}
#[test]
fn 拒绝密码环境且不泄漏密码和路径() {
    let temp = tempfile::tempdir().unwrap();
    let out = temp.path().join("输出.apk");
    let value = result(
        command(temp.path(), None, "sign", Path::new("不存在.apk"), &out)
            .env("MOCIKA_SHIELD_KS_PASS", SECRET)
            .output()
            .unwrap(),
        false,
    );
    assert_eq!(value["code"], "INVALID_REQUEST");
    assert!(!value.to_string().contains(temp.path().to_str().unwrap()));
    assert!(!out.exists());
}
#[test]
fn 签名入口不接受加固参数或用户手工摘要() {
    let temp = tempfile::tempdir().unwrap();
    for flag in [
        "--runtime-profile",
        "--exclude-abis",
        "--shared-binding",
        "--expect-plan-sha256",
    ] {
        let output = command(
            temp.path(),
            None,
            "sign",
            Path::new("a.apk"),
            &temp.path().join("b.apk"),
        )
        .args([flag, "value"])
        .output()
        .unwrap();
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
    }
}

#[test]
#[ignore = "需要真实 JDK、自有 SHIELD_TEST_APK、SHIELD_TEST_APKSIGNER 和 SHIELD_TEST_ZIPALIGN"]
fn 真实一次调用默认和编号签名不需要预览且原包资料不变() {
    let apk = PathBuf::from(std::env::var_os("SHIELD_TEST_APK").expect("指定自有 APK"));
    let signer = PathBuf::from(std::env::var_os("SHIELD_TEST_APKSIGNER").expect("指定签名工具"));
    let align = PathBuf::from(std::env::var_os("SHIELD_TEST_ZIPALIGN").expect("指定 SDK 对齐工具"));
    let before = fs::read(&apk).unwrap();
    let content = shield_core::inspect_signing_input(&apk, || Ok(())).unwrap();
    let temp = tempfile::Builder::new()
        .prefix("受控签名 中文空格")
        .tempdir()
        .unwrap();
    let paths = ApplicationPaths::isolated(temp.path()).unwrap();
    let store =
        CertificateStoreState::initialize(paths.certificate_database(), paths.keystore_dir())
            .unwrap();
    let config = temp.path().join("shield-cli.toml");
    fs::write(
        &config,
        format!(
            "schema_version=1\n[sign]\napksigner={}\n",
            serde_json::to_string(signer.to_str().unwrap()).unwrap()
        ),
    )
    .unwrap();
    let mut previous: Option<PathBuf> = None;
    for kind in ["JKS", "PKCS12"] {
        let cert = create_managed_certificate(
            &store,
            CreateManagedCertificateInput {
                name: kind.into(),
                file_name: kind.into(),
                key_alias: "中文Release".into(),
                keystore_password: SECRET.into(),
                key_password: String::new(),
                ks_type: Some(kind.into()),
                sign_v1: true,
                sign_v2: true,
                sign_v3: true,
                sign_v4: false,
                auto_sign_enabled: false,
                note: "隔离受控签名".into(),
                set_as_default: true,
                dname: "CN=受控签名测试".into(),
                validity_days: 7,
                key_size: 2048,
            },
        )
        .unwrap();
        let database = fs::read(paths.certificate_database()).unwrap();
        let material = fs::read(&cert.keystore_path).unwrap();
        let expected = shield_core::extract_keystore_cert_fingerprint(
            Path::new(&cert.keystore_path),
            &cert.key_alias,
            SECRET,
            Some(kind),
        )
        .unwrap();
        let preview = temp.path().join(format!("预览-{kind}.apk"));
        let plan = result(
            command(temp.path(), Some(&config), "sign-plan", &apk, &preview)
                .output()
                .unwrap(),
            true,
        );
        assert_eq!(plan["plan"]["schema_version"], 4);
        assert_eq!(plan["plan"]["private_key_access_checked"], false);
        assert!(!preview.exists());
        for by_id in [false, true] {
            let out = temp.path().join(format!("独立-{kind}-{by_id}.apk"));
            let mut cmd = command(temp.path(), Some(&config), "sign", &apk, &out);
            if by_id {
                cmd.arg("--certificate-id").arg(&cert.id);
            }
            let value = result(cmd.output().unwrap(), true);
            assert_eq!(value["operation"], "agent_sign");
            assert_eq!(value["status"], "succeeded");
            assert_eq!(value["output_state"], "published");
            assert_eq!(value["cleanup_state"], "complete");
            assert_eq!(value["plan"]["schema_version"], 4);
            assert!(value["plan"].get("shared_binding").is_none());
            assert!(!value.to_string().contains(temp.path().to_str().unwrap()));
            assert_eq!(
                shield_core::inspect_signing_input(&out, || Ok(())).unwrap(),
                content
            );
            let actual = shield_core::extract_apk_cert_fingerprint(&out, Some(&signer)).unwrap();
            assert_eq!(
                shield_core::normalize_fingerprint(&actual),
                shield_core::normalize_fingerprint(&expected)
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
            assert!(shield_core::utils::no_window_command(&align)
                .args(["-c", "-P", "16", "4"])
                .arg(&out)
                .output()
                .unwrap()
                .status
                .success());
            assert!(!PathBuf::from(format!("{}.idsig", out.display())).exists());
            let saved = fs::read(&out).unwrap();
            let rejected = result(
                command(temp.path(), Some(&config), "sign", &apk, &out)
                    .output()
                    .unwrap(),
                false,
            );
            assert_eq!(rejected["code"], "OUTPUT_EXISTS");
            assert_eq!(fs::read(&out).unwrap(), saved);
        }
        let same_input = temp.path().join(format!("独立-{kind}-true.apk"));
        let same_output = temp.path().join(format!("同证书-{kind}.apk"));
        let value = result(
            command(
                temp.path(),
                Some(&config),
                "sign",
                &same_input,
                &same_output,
            )
            .output()
            .unwrap(),
            true,
        );
        assert!(value.get("warnings").is_none());
        if let Some(previous) = &previous {
            let changed = temp.path().join("普通包更换证书.apk");
            let value = result(
                command(temp.path(), Some(&config), "sign", previous, &changed)
                    .output()
                    .unwrap(),
                true,
            );
            assert_eq!(
                value["warnings"],
                serde_json::json!(["SIGNING_CERTIFICATE_CHANGED"])
            );
            assert_eq!(
                shield_core::inspect_signing_input(&changed, || Ok(())).unwrap(),
                content
            );
        }
        previous = Some(same_input);
        assert_eq!(fs::read(paths.certificate_database()).unwrap(), database);
        assert_eq!(fs::read(&cert.keystore_path).unwrap(), material);
    }
    assert_eq!(fs::read(&apk).unwrap(), before);
    assert!(fs::read_dir(temp.path()).unwrap().all(|entry| !entry
        .unwrap()
        .file_name()
        .to_string_lossy()
        .starts_with(".mocika-")));
}

//! 共享身份签名保持原命令事件格式；不代表受控 Agent／MCP 已接入共享库。
use serde_json::Value;
use shield_services::{
    certificates::{
        create_managed_certificate, CertificateStoreState, CreateManagedCertificateInput,
    },
    paths::ApplicationPaths,
};
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
};

const SECRET: &str = "shared-signing-test-password";
fn command(root: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_shield"));
    command
        .arg("--app-home")
        .arg(root)
        .arg("sign")
        .arg("--json")
        .env_remove("MOCIKA_SHIELD_KS_PASS")
        .env_remove("MOCIKA_SHIELD_KEY_PASS");
    command
}
fn error(output: Output) -> Value {
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stderr.is_empty());
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(!text.contains(SECRET));
    let value: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(value["type"], "error");
    value
}

#[test]
fn 共享签名拒绝混合材料且不创建缺失的库() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("未创建");
    let value = error(
        command(&root)
            .args(["--use-default-certificate", "--ks", "临时.p12"])
            .output()
            .unwrap(),
    );
    assert!(value["message"].as_str().unwrap().contains("不能与临时"));
    let value = error(
        command(&root)
            .env("MOCIKA_SHIELD_KS_PASS", SECRET)
            .args(["--certificate-id", "missing"])
            .output()
            .unwrap(),
    );
    assert!(value["message"].as_str().unwrap().contains("不能与临时"));
    let value = error(
        command(&root)
            .args(["--use-default-certificate"])
            .output()
            .unwrap(),
    );
    assert!(value["message"].as_str().unwrap().contains("不会创建空库"));
    assert!(!root.exists());
}

#[test]
fn 损坏项目配置不回显原文或密码() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("shield-cli.toml");
    fs::write(
        &path,
        format!("schema_version=1\n[sign]\nkeystore_password='{SECRET}'\n"),
    )
    .unwrap();
    let value = error(
        command(temp.path())
            .arg("--config")
            .arg(path)
            .arg("--use-default-certificate")
            .output()
            .unwrap(),
    );
    assert!(value["message"]
        .as_str()
        .unwrap()
        .contains("不会输出文件原文"));
    assert!(!temp.path().join("data").exists());
}

#[test]
#[ignore = "需要真实 JDK、SHIELD_TEST_APK 和 SHIELD_TEST_APKSIGNER；仅签名临时输出"]
fn 真实共享证书按编号和默认项独立签名及验签() {
    let apk = PathBuf::from(std::env::var_os("SHIELD_TEST_APK").expect("指定仓库自有 APK"));
    let signer = PathBuf::from(std::env::var_os("SHIELD_TEST_APKSIGNER").expect("指定签名工具"));
    let before = fs::read(&apk).unwrap();
    let temp = tempfile::Builder::new()
        .prefix("共享签名 中文空格")
        .tempdir()
        .unwrap();
    let paths = ApplicationPaths::isolated(temp.path()).unwrap();
    let store =
        CertificateStoreState::initialize(paths.certificate_database(), paths.keystore_dir())
            .unwrap();
    for kind in ["JKS", "PKCS12"] {
        // 与 GUI 共用的服务保存资料，随后命令行不再提供密码。
        let certificate = create_managed_certificate(
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
                note: "隔离回归".into(),
                set_as_default: true,
                dname: "CN=共享签名,O=测试".into(),
                validity_days: 7,
                key_size: 2048,
            },
        )
        .unwrap();
        let keystore_bytes = fs::read(&certificate.keystore_path).unwrap();
        let database_bytes = fs::read(paths.certificate_database()).unwrap();
        for by_id in [true, false] {
            let output = temp.path().join(format!("输出-{kind}-{by_id}.apk"));
            let mut invocation = command(temp.path());
            invocation
                .arg("-i")
                .arg(&apk)
                .arg("-o")
                .arg(&output)
                .arg("--apksigner")
                .arg(&signer);
            if by_id {
                invocation.arg("--certificate-id").arg(&certificate.id);
            } else {
                invocation.arg("--use-default-certificate");
            }
            let result = invocation.output().unwrap();
            assert!(result.status.success(), "共享签名未完成");
            assert!(result.stderr.is_empty());
            let text = String::from_utf8(result.stdout).unwrap();
            assert!(!text.contains(SECRET));
            let events: Vec<Value> = text
                .lines()
                .map(|line| serde_json::from_str(line).unwrap())
                .collect();
            assert_eq!(events.last().unwrap()["type"], "done");
            let expected = shield_core::extract_keystore_cert_fingerprint(
                Path::new(&certificate.keystore_path),
                &certificate.key_alias,
                SECRET,
                Some(&certificate.ks_type),
            )
            .unwrap();
            let actual = shield_core::extract_apk_cert_fingerprint(&output, Some(&signer)).unwrap();
            assert_eq!(
                shield_core::normalize_fingerprint(&expected),
                shield_core::normalize_fingerprint(&actual)
            );
            let verified =
                shield_core::utils::java_command(shield_core::utils::find_java().unwrap())
                    .arg("-jar")
                    .arg(&signer)
                    .arg("verify")
                    .arg(&output)
                    .output()
                    .unwrap();
            assert!(verified.status.success(), "独立验签失败");
            assert_eq!(fs::read(&apk).unwrap(), before);
            assert_eq!(
                fs::read(&certificate.keystore_path).unwrap(),
                keystore_bytes
            );
            assert_eq!(
                fs::read(paths.certificate_database()).unwrap(),
                database_bytes
            );
        }
    }
}

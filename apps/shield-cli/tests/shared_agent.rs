//! 共享受控命令专项；使用隔离资料，不读取或写入维护者配置。
#![cfg(all(feature = "agent-executor", target_os = "macos"))]
use serde_json::{json, Value};
use shield_app::{
    certificates::{CertificatePreferencesPatch, CertificateStoreState, CertificateUpsertInput},
    config::ConfigStore,
    paths::ApplicationPaths,
};
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
};
#[path = "support/execution_fixture.rs"]
mod fixture;
use fixture::Fixture;

fn draft(name: &str) -> CertificateUpsertInput {
    CertificateUpsertInput {
        id: None,
        name: name.into(),
        source_type: "external".into(),
        keystore_path: String::new(),
        keystore_password: Fixture::password().into(),
        key_alias: "agentfixture".into(),
        key_password: String::new(),
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
fn terminal(output: Output) -> Value {
    assert!(output.stderr.is_empty());
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(!text.contains(Fixture::password()));
    let values: Vec<Value> = text
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(values.first().unwrap()["type"], "started");
    assert_eq!(values.iter().filter(|v| v["type"] == "result").count(), 1);
    let value = values.last().unwrap().clone();
    assert_eq!(value["type"], "result");
    assert_eq!(
        output.status.success(),
        matches!(
            value["status"].as_str(),
            Some("ready" | "warning" | "succeeded")
        ),
        "有限终态：{value}"
    );
    assert_eq!(value["retryable"], false);
    value
}
fn command(fixture: &Fixture, home: &Path, operation: &str, output: &Path) -> Command {
    let bundle = std::env::var_os("SHIELD_SHARED_TEST_APP").map(PathBuf::from);
    let program = bundle
        .as_ref()
        .map(|app| {
            assert!(app.is_absolute() && app.is_dir());
            app.join("Contents/MacOS/shield")
        })
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_BIN_EXE_shield")));
    let mut command = fixture.command(&program);
    command
        .env_remove("MOCIKA_SHIELD_KS_PASS")
        .env_remove("MOCIKA_SHIELD_KEY_PASS")
        .arg("--app-home")
        .arg(home)
        .args(["agent", operation, "-i"])
        .arg(&fixture.input)
        .arg("-o")
        .arg(output);
    if bundle.is_some() {
        command.current_dir(fixture.root());
    }
    command
}
fn protect(fixture: &Fixture, home: &Path, output: &Path, plan: &Value) -> Command {
    let mut command = command(fixture, home, "protect", output);
    command
        .arg("--shared-binding")
        .arg(
            plan["plan"]["shared_binding"]
                .as_str()
                .expect("共享计划必须返回绑定"),
        )
        .arg("--expect-plan-sha256")
        .arg(plan["plan"]["sha256"].as_str().unwrap());
    command
}

#[test]
fn 无效绑定和缺失绑定在启动前拒绝且不创建资料() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("不存在的应用目录");
    for token in [None, Some("not-valid")] {
        let mut command = Command::new(env!("CARGO_BIN_EXE_shield"));
        command
            .arg("--app-home")
            .arg(&home)
            .args([
                "agent",
                "protect",
                "-i",
                "missing.apk",
                "-o",
                "output.apk",
                "--expect-plan-sha256",
            ])
            .arg("00".repeat(32));
        if let Some(token) = token {
            command.args(["--shared-binding", token]);
        }
        let value = terminal(command.output().unwrap());
        assert_eq!(value["code"], "SHARED_BINDING_INVALID");
    }
    assert!(!home.exists());
}

#[test]
#[ignore = "需要显式自有 SHIELD_AGENT_TEST_APK、现有 JDK 和双资源；不纳入日常 CI"]
fn 真实共享受控双模式默认切换及材料变更() {
    let fixture = Fixture::new();
    let zipalign =
        std::env::var_os("SHIELD_AGENT_TEST_ZIPALIGN").expect("指定 SDK zipalign 独立核对对齐");
    let package_hashes = std::env::var_os("SHIELD_SHARED_TEST_APP")
        .map(PathBuf::from)
        .map(|app| {
            [
                "Contents/MacOS/shield",
                "Contents/MacOS/mocika-shield",
                "Contents/Resources/tools/apktool.jar",
                "Contents/Resources/tools/apksigner.jar",
                "Contents/Resources/resources/resources.zip",
                "Contents/Resources/resources/resources-api19.zip",
            ]
            .into_iter()
            .map(|name| {
                let path = app.join(name);
                let hash = fixture::hash(&path);
                (path, hash)
            })
            .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let external_config_hash = fixture::hash(&fixture.config);
    let home = fixture.root().join("共享资料 中文");
    let paths = ApplicationPaths::isolated(&home).unwrap();
    let store =
        CertificateStoreState::initialize(paths.certificate_database(), paths.keystore_dir())
            .unwrap();
    let keystore = fixture.root().join("临时测试证书.p12");
    let original_keystore = fs::read(&keystore).unwrap();
    let certificate = store
        .save_certificate(
            &draft("受控原证书"),
            keystore.to_str().unwrap(),
            "agentfixture",
            None,
        )
        .unwrap();
    let expected =
        shield_core::extract_apk_cert_fingerprint(&fixture.input, Some(&fixture.signer)).unwrap();
    let output = fixture.output.clone();
    let plan = terminal(
        command(&fixture, &home, "plan", &output)
            .arg("--use-default-certificate")
            .output()
            .unwrap(),
    );
    assert_eq!(plan["code"], "OK", "有限终态：{plan}");
    assert_eq!(plan["plan"]["schema_version"], 3);
    let config = ConfigStore::new(paths.config_file());
    config
        .update_protect_defaults(
            serde_json::from_value(
                json!({"runtime_mode":"android_api19","environment_policy":"strict"}),
            )
            .unwrap(),
        )
        .unwrap();
    store
        .patch_certificate_preferences(
            &certificate.id,
            CertificatePreferencesPatch {
                name: Some("已改名".into()),
                note: Some("无关变化".into()),
                ..Default::default()
            },
            false,
        )
        .unwrap();
    store
        .update_verify_status(&certificate.id, "success", Some("新的校验时间"), None)
        .unwrap();
    let unused = fixture.root().join("不会使用的证书.p12");
    fs::write(&unused, b"not-keystore").unwrap();
    store
        .save_certificate(
            &draft("新的默认项"),
            unused.to_str().unwrap(),
            "unused",
            None,
        )
        .unwrap();
    let before_db = fs::read(paths.certificate_database()).unwrap();
    let before_config = fs::read(paths.config_file()).unwrap();
    let result = terminal(protect(&fixture, &home, &output, &plan).output().unwrap());
    assert_eq!(result["status"], "succeeded", "有限终态：{result}");
    assert_eq!(result["plan"]["sha256"], plan["plan"]["sha256"]);
    assert_eq!(
        shield_core::normalize_fingerprint(
            &shield_core::extract_apk_cert_fingerprint(&output, Some(&fixture.signer)).unwrap()
        ),
        shield_core::normalize_fingerprint(&expected)
    );
    assert_eq!(fs::read(paths.certificate_database()).unwrap(), before_db);
    assert_eq!(fs::read(paths.config_file()).unwrap(), before_config);
    // 已存在的最终文件不得被同一绑定重放覆盖。
    let output_hash = fixture::hash(&output);
    let repeat = terminal(protect(&fixture, &home, &output, &plan).output().unwrap());
    assert_ne!(repeat["status"], "succeeded");
    assert_eq!(fixture::hash(&output), output_hash);

    // 有项目配置的工控规划使用新版六角色，固定显式版本覆盖。
    let project = fixture.root().join("shared-project.toml");
    fs::write(&project, "schema_version=1\n[sign]\nv3=false\n").unwrap();
    let api19_output = fixture.root().join("工控受控输出.apk");
    let api19_plan = terminal(
        command(&fixture, &home, "plan", &api19_output)
            .arg("--config")
            .arg(&project)
            .arg("--certificate-id")
            .arg(&certificate.id)
            .output()
            .unwrap(),
    );
    assert_eq!(api19_plan["code"], "OK", "有限终态：{api19_plan}");
    let result = terminal(
        protect(&fixture, &home, &api19_output, &api19_plan)
            .arg("--config")
            .arg(&project)
            .output()
            .unwrap(),
    );
    assert_eq!(result["status"], "succeeded", "有限终态：{result}");
    assert_eq!(
        shield_core::normalize_fingerprint(
            &shield_core::extract_apk_cert_fingerprint(&api19_output, Some(&fixture.signer))
                .unwrap()
        ),
        shield_core::normalize_fingerprint(&expected)
    );

    let failed = fixture.root().join("材料变化不得交付.apk");
    let plan = terminal(
        command(&fixture, &home, "plan", &failed)
            .arg("--certificate-id")
            .arg(&certificate.id)
            .output()
            .unwrap(),
    );
    store
        .patch_certificate_preferences(
            &certificate.id,
            CertificatePreferencesPatch {
                sign_v3: Some(false),
                ..Default::default()
            },
            false,
        )
        .unwrap();
    let result = terminal(protect(&fixture, &home, &failed, &plan).output().unwrap());
    assert_eq!(result["code"], "PLAN_SOURCE_CHANGED", "有限终态：{result}");
    assert!(!failed.exists());
    store
        .patch_certificate_preferences(
            &certificate.id,
            CertificatePreferencesPatch {
                sign_v3: Some(true),
                ..Default::default()
            },
            false,
        )
        .unwrap();
    store.delete_certificate(&certificate.id, false).unwrap();
    let result = terminal(protect(&fixture, &home, &failed, &plan).output().unwrap());
    assert_eq!(result["code"], "PLAN_SOURCE_CHANGED");
    assert!(!failed.exists());
    assert_eq!(fs::read(keystore).unwrap(), original_keystore);
    for output in [&output, &api19_output] {
        assert!(
            shield_core::utils::no_window_command(&zipalign)
                .args(["-c", "-P", "16", "4"])
                .arg(output)
                .output()
                .unwrap()
                .status
                .success(),
            "SDK 独立对齐核对失败"
        );
    }
    for (path, expected) in package_hashes {
        assert_eq!(fixture::hash(&path), expected, "任务不得改写应用包");
    }
    fixture.assert_inputs_unchanged();
    assert_eq!(fixture::hash(&fixture.config), external_config_hash);
    fixture.assert_no_tasks();
}

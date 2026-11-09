//! 标准流真实专项；可显式选完整本地应用包，不等于实际 Agent 宿主注册或公开安装包验收。
use super::*;
use shield_app::certificates::{
    create_managed_certificate, CertificatePreferencesPatch, CreateManagedCertificateInput,
};
use std::{
    os::unix::fs::PermissionsExt,
    time::{Duration, Instant},
};

fn setup(fixture: &Fixture) -> (PathBuf, Vec<u8>) {
    let input = PathBuf::from(std::env::var_os("SHIELD_TEST_APK").expect("指定仓库自有 APK"));
    let signer = PathBuf::from(std::env::var_os("SHIELD_TEST_APKSIGNER").expect("指定签名工具"));
    let before = fs::read(&input).unwrap();
    fs::copy(input, fixture.root.path().join("输入/应用.apk")).unwrap();
    fs::write(
        fixture.root.path().join("shield-cli.toml"),
        format!(
            "schema_version=1\n[sign]\napksigner={}\n",
            serde_json::to_string(signer.to_str().unwrap()).unwrap()
        ),
    )
    .unwrap();
    (signer, before)
}
fn certificate(
    store: &CertificateStoreState,
    kind: &str,
) -> shield_app::certificates::CertificateRecord {
    create_managed_certificate(
        store,
        CreateManagedCertificateInput {
            name: format!("真实-{kind}"),
            file_name: format!("真实-{kind}"),
            key_alias: "中文别名".into(),
            keystore_password: SECRET.into(),
            key_password: String::new(),
            ks_type: Some(kind.into()),
            sign_v1: true,
            sign_v2: true,
            sign_v3: true,
            sign_v4: false,
            auto_sign_enabled: false,
            note: "私有备注".into(),
            set_as_default: true,
            dname: "CN=本地协议签名专项".into(),
            validity_days: 7,
            key_size: 2048,
        },
    )
    .unwrap()
}
fn sign(session: &mut Session, id: u64, args: Value) -> Value {
    session.send(json!({"jsonrpc":"2.0","id":id,"method":"tools/call","params":{"name":"shield_sign","arguments":args,"_meta":{"progressToken":format!("签名-{id}")}}}));
    session.receive(id)["result"]["structuredContent"].clone()
}
fn verified(fixture: &Fixture, result: &Value, name: &str, signer: &Path, expected: &str) {
    assert_eq!(result["status"], "succeeded", "有限签名结果：{result}");
    assert_eq!(result["operation"], "agent_sign");
    assert_eq!(result["output_state"], "published");
    assert_eq!(result["plan"]["schema_version"], 4);
    assert_eq!(result["cleanup_state"], "complete");
    assert_eq!(result["session_execution_blocked"], false);
    private(result, fixture);
    verified_output(fixture, name, signer, expected);
}
fn verified_output(fixture: &Fixture, name: &str, signer: &Path, expected: &str) {
    let output = fixture.root.path().join("输出").join(name);
    assert_eq!(
        shield_core::normalize_fingerprint(
            &shield_core::extract_apk_cert_fingerprint(&output, Some(signer)).unwrap()
        ),
        shield_core::normalize_fingerprint(expected)
    );
    assert!(
        shield_core::utils::java_command(shield_core::utils::find_java().unwrap())
            .arg("-jar")
            .arg(signer)
            .arg("verify")
            .arg(&output)
            .output()
            .unwrap()
            .status
            .success()
    );
    let align = std::env::var_os("SHIELD_TEST_ZIPALIGN").expect("指定 SDK zipalign");
    assert!(shield_core::utils::no_window_command(align)
        .args(["-c", "-P", "16", "4"])
        .arg(&output)
        .output()
        .unwrap()
        .status
        .success());
    assert_eq!(
        shield_core::inspect_signing_input(&output, || Ok(())).unwrap(),
        shield_core::inspect_signing_input(&fixture.root.path().join("输入/应用.apk"), || Ok(
            ()
        ))
        .unwrap()
    );
    assert!(!PathBuf::from(format!("{}.idsig", output.display())).exists());
}
#[test]
#[ignore = "需要真实 JDK、自有 SHIELD_TEST_APK、SHIELD_TEST_APKSIGNER 及 SHIELD_TEST_ZIPALIGN；完整包可由 SHIELD_SHARED_TEST_APP 指定"]
fn 单次真实签名覆盖默认显式唯一范围换证书和错误后继续使用() {
    let fixture = Fixture::new();
    let (signer, before) = setup(&fixture);
    let store = fixture.store();
    let first = certificate(&store, "JKS");
    let second = certificate(&store, "PKCS12");
    let expected = |cert: &shield_app::certificates::CertificateRecord| {
        shield_core::extract_keystore_cert_fingerprint(
            Path::new(&cert.keystore_path),
            &cert.key_alias,
            SECRET,
            Some(&cert.ks_type),
        )
        .unwrap()
    };
    let first_fp = expected(&first);
    let second_fp = expected(&second);
    let first_bytes = fs::read(&first.keystore_path).unwrap();
    let second_bytes = fs::read(&second.keystore_path).unwrap();
    // 同一完整包的公开 CLI 在关闭标准输入时也必须单次执行，无需 MCP 前置计划。
    let direct = fixture
        .cli_command()
        .arg("--app-home")
        .arg(fixture.home())
        .args(["agent", "sign", "--timeout-seconds", "60", "-i"])
        .arg(fixture.root.path().join("输入/应用.apk"))
        .arg("-o")
        .arg(fixture.root.path().join("输出/命令行签名.apk"))
        .stdin(std::process::Stdio::null())
        .env_remove("MOCIKA_SHIELD_KS_PASS")
        .env_remove("MOCIKA_SHIELD_KEY_PASS")
        .env_remove("JAVA_TOOL_OPTIONS")
        .env_remove("_JAVA_OPTIONS")
        .env_remove("JDK_JAVA_OPTIONS")
        .output()
        .unwrap();
    assert!(direct.status.success());
    assert!(direct.stderr.is_empty());
    let events = String::from_utf8(direct.stdout)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(events.iter().filter(|v| v["type"] == "result").count(), 1);
    let result = events.last().unwrap();
    assert_eq!(result["operation"], "agent_sign");
    assert_eq!(result["status"], "succeeded");
    assert_eq!(result["plan"]["schema_version"], 4);
    assert_eq!(result["cleanup_state"], "complete");
    private(result, &fixture);
    verified_output(&fixture, "命令行签名.apk", &signer, &second_fp);
    store
        .patch_certificate_preferences(
            &second.id,
            CertificatePreferencesPatch {
                sign_v4: Some(true),
                ..Default::default()
            },
            false,
        )
        .unwrap();
    let mut session = Session::start(fixture.command(&[&first.id, &second.id], true, true, 60));
    let blocked = sign(
        &mut session,
        2,
        json!({"input":"应用.apk","output":"默认签名.apk"}),
    );
    assert_eq!(blocked["code"], "SIGNING_V4_NOT_SUPPORTED");
    assert_eq!(blocked["session_execution_blocked"], false);
    store
        .patch_certificate_preferences(
            &second.id,
            CertificatePreferencesPatch {
                sign_v4: Some(false),
                ..Default::default()
            },
            false,
        )
        .unwrap();
    let result = sign(
        &mut session,
        3,
        json!({"input":"应用.apk","output":"默认签名.apk"}),
    );
    verified(&fixture, &result, "默认签名.apk", &signer, &second_fp);
    let progress = session
        .notifications
        .iter()
        .filter(|n| n["params"]["progressToken"] == "签名-3")
        .collect::<Vec<_>>();
    assert!(!progress.is_empty());
    assert!(progress
        .iter()
        .all(|n| n["params"]["_meta"]["mocika-shield/plan_id"] == result["plan_id"]));
    let status = session.call(
        4,
        "shield_execution_status",
        json!({"plan_id":result["plan_id"]}),
    );
    assert_eq!(status["state"], "finished");
    assert_eq!(status["operation"], "agent_sign");
    private(&status, &fixture);
    assert_eq!(
        session.call(
            5,
            "shield_protect",
            json!({"plan_id":result["plan_id"],"expect_plan_sha256":"0".repeat(64)})
        )["code"],
        "PLAN_NOT_AVAILABLE"
    );
    let result = sign(
        &mut session,
        6,
        json!({"input":"应用.apk","output":"编号签名.apk","certificate_id":first.id}),
    );
    verified(&fixture, &result, "编号签名.apk", &signer, &first_fp);
    fs::copy(
        fixture.root.path().join("输出/默认签名.apk"),
        fixture.root.path().join("输入/已签名.apk"),
    )
    .unwrap();
    let result = sign(
        &mut session,
        7,
        json!({"input":"已签名.apk","output":"更换证书.apk","certificate_id":first.id}),
    );
    verified(&fixture, &result, "更换证书.apk", &signer, &first_fp);
    assert_eq!(result["warnings"], json!(["SIGNING_CERTIFICATE_CHANGED"]));
    let _outside = fixture.fake(&store, "其他默认", false);
    let database = fs::read(
        ApplicationPaths::isolated(&fixture.home())
            .unwrap()
            .certificate_database(),
    )
    .unwrap();
    let ambiguous = sign(
        &mut session,
        8,
        json!({"input":"应用.apk","output":"不能猜选.apk"}),
    );
    assert_eq!(ambiguous["code"], "CERTIFICATE_REQUIRED");
    assert_eq!(ambiguous["session_execution_blocked"], false);
    let result = sign(
        &mut session,
        9,
        json!({"input":"应用.apk","output":"明确选择后.apk","certificate_id":first.id}),
    );
    verified(&fixture, &result, "明确选择后.apk", &signer, &first_fp);
    assert_eq!(
        sign(
            &mut session,
            10,
            json!({"input":"应用.apk","output":"明确选择后.apk","certificate_id":first.id})
        )["code"],
        "OUTPUT_EXISTS"
    );
    session.close();
    let mut session = Session::start(fixture.command(&[&first.id], true, true, 60));
    let result = sign(
        &mut session,
        2,
        json!({"input":"应用.apk","output":"唯一允许项.apk"}),
    );
    verified(&fixture, &result, "唯一允许项.apk", &signer, &first_fp);
    session.close();
    assert_eq!(
        fs::read(
            ApplicationPaths::isolated(&fixture.home())
                .unwrap()
                .certificate_database()
        )
        .unwrap(),
        database
    );
    assert_eq!(fs::read(&first.keystore_path).unwrap(), first_bytes);
    assert_eq!(fs::read(&second.keystore_path).unwrap(), second_bytes);
    assert_eq!(
        fs::read(fixture.root.path().join("输入/应用.apk")).unwrap(),
        before
    );
    fixture.unchanged_tasks();
}

#[test]
#[ignore = "需要真实 JDK、自有 SHIELD_TEST_APK 和 SHIELD_TEST_APKSIGNER，真实挂起签名验证收尾"]
fn 单次签名运行中状态忙取消超时和断开均不遗留产物或后代() {
    for mode in ["cancel", "timeout", "closed"] {
        let fixture = Fixture::new();
        let (_signer, before) = setup(&fixture);
        let store = fixture.store();
        let cert = certificate(&store, "PKCS12");
        let bin = fixture.root.path().join("bin");
        fs::create_dir(&bin).unwrap();
        let java = shield_core::utils::find_java().unwrap();
        let keytool = shield_core::utils::find_keytool().unwrap();
        std::os::unix::fs::symlink(keytool, bin.join("keytool")).unwrap();
        fs::write(bin.join("java"),r#"#!/bin/sh
case " $* " in
  *'/materials/apksigner.jar sign '*)
    /bin/sh -c 'while :; do printf x >> "$SHIELD_SIGN_GATE/child-heartbeat"; /bin/sleep 0.02; done' &
    while :; do printf x >> "$SHIELD_SIGN_GATE/heartbeat"; /bin/sleep 0.02; done
    ;;
esac
exec "$SHIELD_SIGN_REAL_JAVA" "$@"
"#).unwrap();
        fs::set_permissions(bin.join("java"), fs::Permissions::from_mode(0o700)).unwrap();
        let mut cmd = fixture.command(&[&cert.id], true, true, 30);
        cmd.env("PATH", bin)
            .env_remove("JAVA_HOME")
            .env("SHIELD_SIGN_REAL_JAVA", java)
            .env("SHIELD_SIGN_GATE", fixture.root.path());
        let mut session = Session::start(cmd);
        session.send(json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"shield_sign","arguments":{"input":"应用.apk","output":"不应交付.apk"},"_meta":{"progressToken":"控制专项"}}}));
        session.wait_stage("sign");
        let id = session
            .notifications
            .iter()
            .rev()
            .find(|n| n["params"]["message"] == "sign")
            .unwrap()["params"]["_meta"]["mocika-shield/plan_id"]
            .as_str()
            .unwrap()
            .to_owned();
        let until = Instant::now() + Duration::from_secs(15);
        while !["heartbeat", "child-heartbeat"].iter().all(|n| {
            fixture
                .root
                .path()
                .join(n)
                .metadata()
                .is_ok_and(|m| m.len() > 0)
        }) {
            assert!(Instant::now() < until, "真实签名未到达故障门槛");
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(
            session.call(
                4,
                "shield_sign",
                json!({"input":"应用.apk","output":"第二项.apk"})
            )["code"],
            "SERVER_BUSY"
        );
        let status = session.call(5, "shield_execution_status", json!({"plan_id":id}));
        assert_eq!(status["state"], "running");
        assert_eq!(status["operation"], "agent_sign");
        private(&status, &fixture);
        if mode == "closed" {
            session.close();
        } else {
            if mode == "cancel" {
                session.send(json!({"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":3,"reason":"隔离回归取消"}}));
            }
            let until = Instant::now() + Duration::from_secs(45);
            loop {
                let status = session.call(6, "shield_execution_status", json!({"plan_id":id}));
                private(&status, &fixture);
                if status["state"] == "finished" {
                    assert_eq!(
                        status["status"],
                        if mode == "timeout" {
                            "timed_out"
                        } else {
                            "cancelled"
                        }
                    );
                    assert_eq!(status["cleanup_state"], "complete");
                    assert_eq!(status["session_execution_blocked"], false);
                    break;
                }
                assert!(Instant::now() < until, "签名未按期限形成终态");
                std::thread::sleep(Duration::from_millis(50));
            }
            session.close();
        }
        for name in ["heartbeat", "child-heartbeat"] {
            let bytes = fs::read(fixture.root.path().join(name)).unwrap();
            std::thread::sleep(Duration::from_millis(100));
            assert_eq!(fs::read(fixture.root.path().join(name)).unwrap(), bytes);
        }
        assert!(!fixture.root.path().join("输出/不应交付.apk").exists());
        assert!(!fixture.root.path().join("输出/第二项.apk").exists());
        assert_eq!(
            fs::read(fixture.root.path().join("输入/应用.apk")).unwrap(),
            before
        );
        fixture.unchanged_tasks();
    }
}

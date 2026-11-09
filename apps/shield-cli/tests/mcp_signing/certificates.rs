//! 证书生命周期真实标准流；资料与密码全部为本次隔离合成值。
use super::*;

fn create(name: &str) -> Value {
    json!({"name":name,"file_name":"发布证书.p12","alias":"中文Alias","dname":"CN=生命周期测试,O=本机隔离","keystore_type":"pkcs12","set_default":true})
}
#[test]
fn 缺宿主密码不索取对话口令不创建资料且无额外确认参数() {
    let fixture = Fixture::new();
    let mut command = fixture.command(&[], true, true, 120);
    command
        .env_remove("MOCIKA_SHIELD_KS_PASS")
        .env_remove("MOCIKA_SHIELD_KEY_PASS");
    let mut session = Session::start(command);
    let value = session.call(2, "shield_create_certificate", create("缺凭据测试"));
    assert_eq!(value["code"], "CREDENTIAL_REQUIRED");
    assert_eq!(value["change_state"], "not_attempted");
    assert_eq!(value["session_execution_blocked"], false);
    assert!(!fixture.home().exists());
    for name in ["shield_verify_certificate", "shield_remove_certificate"] {
        let value = session.call(3, name, json!({"certificate_id":"other"}));
        assert_eq!(value["code"], "CERTIFICATE_NOT_ALLOWED");
        assert_eq!(value["change_state"], "not_attempted");
    }
    let mut with_password = create("禁止对话密码");
    with_password["password"] = json!("不得发送");
    assert_eq!(
        session.call(4, "shield_create_certificate", with_password)["code"],
        "ARGUMENT_INVALID"
    );
    session.close();
    fixture.unchanged_tasks();
}

#[test]
#[ignore = "需显式提供自有 APK、独立 apksigner 与 SDK zipalign，可使用完整应用包"]
fn 真实创建导入校验签名删除在同会话完成且文件保留() {
    let fixture = Fixture::new();
    let apk = PathBuf::from(std::env::var_os("SHIELD_TEST_APK").expect("需指定自有 APK"));
    let signer =
        PathBuf::from(std::env::var_os("SHIELD_TEST_APKSIGNER").expect("需指定独立验签工具"));
    let align =
        PathBuf::from(std::env::var_os("SHIELD_TEST_ZIPALIGN").expect("需指定 SDK zipalign"));
    fs::copy(&apk, fixture.root.path().join("输入/应用.apk")).unwrap();
    let input_hash = package_hash(&apk);
    let mut command = fixture.command(&[], true, true, 120);
    command
        .env("MOCIKA_SHIELD_KS_PASS", SECRET)
        .env_remove("MOCIKA_SHIELD_KEY_PASS");
    let mut session = Session::start(command);
    let created = session.call(2, "shield_create_certificate", create("合成测试证书"));
    assert_eq!(created["status"], "ready", "{created}");
    private(&created, &fixture);
    let first = created["data"]["certificate"]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let store = fixture.store();
    let first_record = store.get_certificate(&first).unwrap().unwrap();
    assert_eq!(first_record.keystore_password, SECRET);
    assert!(first_record.is_default);
    let material = fs::read(&first_record.keystore_path).unwrap();
    let verified = session.call(
        3,
        "shield_verify_certificate",
        json!({"certificate_id":first}),
    );
    assert_eq!(
        verified["data"]["certificate"]["verification_status"],
        "success"
    );
    assert_eq!(verified["change_state"], "confirmed");
    // 原地损坏本次合成材料，校验失败但状态保存成功不应锁死整个会话。
    fs::write(&first_record.keystore_path, b"invalid-keystore").unwrap();
    let failed = session.call(
        4,
        "shield_verify_certificate",
        json!({"certificate_id":first}),
    );
    assert_eq!(failed["status"], "blocked");
    assert_eq!(failed["change_state"], "confirmed");
    assert_eq!(
        failed["data"]["certificate"]["verification_status"],
        "failed"
    );
    assert_eq!(failed["session_execution_blocked"], false);
    assert_eq!(
        store
            .get_certificate(&first)
            .unwrap()
            .unwrap()
            .last_verify_status,
        "failed"
    );
    private(&failed, &fixture);
    fs::write(&first_record.keystore_path, &material).unwrap();
    assert_eq!(
        session.call(
            5,
            "shield_verify_certificate",
            json!({"certificate_id":first})
        )["status"],
        "ready"
    );
    let signed = session.call(
        6,
        "shield_sign",
        json!({"input":"应用.apk","output":"新建证书签名.apk"}),
    );
    assert_eq!(signed["status"], "succeeded", "{signed}");
    private(&signed, &fixture);
    check_apk(
        &fixture.root.path().join("输出/新建证书签名.apk"),
        &signer,
        &align,
        &first_record,
    );
    let source = fixture.root.path().join("输入/导入证书.p12");
    fs::write(&source, &material).unwrap();
    let imported = session.call(
        7,
        "shield_import_certificate",
        json!({"name":"合成导入","keystore":"导入证书.p12","keystore_type":"pkcs12"}),
    );
    assert_eq!(imported["status"], "ready", "{imported}");
    private(&imported, &fixture);
    let second = imported["data"]["certificate"]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let second_record = store.get_certificate(&second).unwrap().unwrap();
    assert_eq!(
        fs::canonicalize(&second_record.keystore_path).unwrap(),
        fs::canonicalize(&source).unwrap()
    );
    assert!(second_record.key_alias.eq_ignore_ascii_case("中文Alias"));
    assert_eq!(second_record.keystore_password, SECRET);
    let listing = session.call(8, "shield_certificates", json!({}));
    assert_eq!(listing["data"]["certificates"].as_array().unwrap().len(), 2);
    assert_eq!(
        session.call(
            9,
            "shield_set_default_certificate",
            json!({"certificate_id":second})
        )["status"],
        "ready"
    );
    let signed = session.call(
        10,
        "shield_sign",
        json!({"input":"应用.apk","output":"导入证书签名.apk"}),
    );
    assert_eq!(signed["status"], "succeeded", "{signed}");
    check_apk(
        &fixture.root.path().join("输出/导入证书签名.apk"),
        &signer,
        &align,
        &second_record,
    );
    // 同名创建不得覆盖第一份；第三个编号也须在同会话可删除。
    let duplicate = session.call(11, "shield_create_certificate", create("同名合成证书"));
    assert_eq!(duplicate["status"], "ready", "{duplicate}");
    let third = duplicate["data"]["certificate"]["id"].as_str().unwrap();
    let third_record = store.get_certificate(third).unwrap().unwrap();
    assert_ne!(third_record.keystore_path, first_record.keystore_path);
    for id in [&first, &second, third] {
        let removed = session.call(
            12,
            "shield_remove_certificate",
            json!({"certificate_id":id}),
        );
        assert_eq!(removed["status"], "ready");
        assert_eq!(removed["data"]["keystore_file"], "retained");
        assert!(store.get_certificate(id).unwrap().is_none());
        assert_eq!(
            session.call(13, "shield_certificates", json!({"certificate_id":id}))["code"],
            "CERTIFICATE_NOT_ALLOWED"
        );
    }
    assert!(store.list_certificates().unwrap().is_empty());
    assert_eq!(fs::read(&first_record.keystore_path).unwrap(), material);
    assert_eq!(fs::read(&source).unwrap(), material);
    assert!(Path::new(&third_record.keystore_path).is_file());
    assert_eq!(package_hash(&apk), input_hash);
    session.close();
    fixture.unchanged_tasks();
}

fn check_apk(
    path: &Path,
    signer: &Path,
    align: &Path,
    record: &shield_services::certificates::CertificateRecord,
) {
    let output = shield_core::utils::java_command(shield_core::utils::find_java().unwrap())
        .args(["-jar"])
        .arg(signer)
        .args(["verify", "--verbose", "--print-certs"])
        .arg(path)
        .output()
        .unwrap();
    assert!(output.status.success());
    let text = String::from_utf8(output.stdout).unwrap();
    for version in ["v1", "v2", "v3"] {
        assert!(text.lines().any(
            |line| line.contains(&format!("using {version} scheme")) && line.ends_with("true")
        ));
    }
    let fingerprint = shield_core::extract_keystore_cert_fingerprint(
        Path::new(&record.keystore_path),
        &record.key_alias,
        &record.keystore_password,
        Some(&record.ks_type),
    );
    // 独立验签输出必须对应本次保存证书，而不只检查退出状态。
    let fingerprint = fingerprint.unwrap();
    assert_eq!(
        shield_core::normalize_fingerprint(
            &shield_core::extract_apk_cert_fingerprint(path, Some(signer)).unwrap()
        ),
        shield_core::normalize_fingerprint(&fingerprint)
    );
    assert!(!PathBuf::from(format!("{}.idsig", path.display())).exists());
    assert!(Command::new(align)
        .args(["-c", "-P", "16", "4"])
        .arg(path)
        .status()
        .unwrap()
        .success());
}

#[test]
#[ignore = "需真实 JDK，显式验证创建阶段取消／超时／断开及子进程停止"]
fn 创建运行中取消超时断开不遗留后代且未知结果不能重试() {
    use std::{
        os::unix::fs::PermissionsExt,
        time::{Duration, Instant},
    };
    for mode in ["cancel", "timeout", "closed"] {
        let fixture = Fixture::new();
        let bin = fixture.root.path().join("bin");
        fs::create_dir(&bin).unwrap();
        let java = shield_core::utils::find_java().unwrap();
        let keytool = shield_core::utils::find_keytool().unwrap();
        // 使用真实普通启动器，确保关联 Java 的同目录 keytool 就是本次故障注入工具。
        // 若将 java 链接到真实 JDK，探测会正确选取真实 JDK 的 keytool，从而绕过夹具。
        fs::write(
            bin.join("java"),
            "#!/bin/sh\nexec \"$SHIELD_CERT_REAL_JAVA\" \"$@\"\n",
        )
        .unwrap();
        fs::set_permissions(bin.join("java"), fs::Permissions::from_mode(0o700)).unwrap();
        fs::write(bin.join("keytool"),r#"#!/bin/sh
case " $* " in
  *' -genkeypair '*)
    /bin/sh -c 'while :; do printf x >> "$SHIELD_CERT_GATE/child-heartbeat"; /bin/sleep 0.02; done' &
    while :; do printf x >> "$SHIELD_CERT_GATE/heartbeat"; /bin/sleep 0.02; done
    ;;
esac
exec "$SHIELD_CERT_REAL_KEYTOOL" "$@"
"#).unwrap();
        fs::set_permissions(bin.join("keytool"), fs::Permissions::from_mode(0o700)).unwrap();
        let mut command = fixture.command(&[], true, true, 20);
        command
            .env("PATH", bin)
            .env_remove("JAVA_HOME")
            .env("MOCIKA_SHIELD_KS_PASS", SECRET)
            .env_remove("MOCIKA_SHIELD_KEY_PASS")
            .env("SHIELD_CERT_GATE", fixture.root.path())
            .env("SHIELD_CERT_REAL_JAVA", java)
            .env("SHIELD_CERT_REAL_KEYTOOL", keytool);
        let mut session = Session::start(command);
        session.send(json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"shield_create_certificate","arguments":create("停止专项")}}));
        let until = Instant::now() + Duration::from_secs(15);
        while !["heartbeat", "child-heartbeat"].iter().all(|name| {
            fixture
                .root
                .path()
                .join(name)
                .metadata()
                .is_ok_and(|m| m.len() > 0)
        }) {
            assert!(Instant::now() < until, "创建未到达挂起门槛");
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(
            session.call(
                4,
                "shield_remove_certificate",
                json!({"certificate_id":"other"})
            )["code"],
            "SERVER_BUSY"
        );
        if mode == "closed" {
            session.close();
        } else {
            if mode == "cancel" {
                session.send(json!({"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":3,"reason":"本次隔离创建取消"}}));
            } else {
                let result = session.receive(3)["result"]["structuredContent"].clone();
                assert_eq!(result["code"], "TASK_TIMEOUT");
                assert_eq!(result["change_state"], "unconfirmed");
            }
            let until = Instant::now() + Duration::from_secs(30);
            loop {
                let read = session.call(5, "shield_settings", json!({}));
                if read["code"] != "SERVER_BUSY" {
                    assert_eq!(read["status"], "ready");
                    assert_eq!(read["session_execution_blocked"], true);
                    break;
                }
                assert!(Instant::now() < until);
                std::thread::sleep(Duration::from_millis(20));
            }
            assert_eq!(
                session.call(6, "shield_create_certificate", create("不可自动重试"))["code"],
                "SESSION_RECOVERY_REQUIRED"
            );
            assert_eq!(
                session.call(7, "shield_certificates", json!({}))["data"]["certificates"],
                json!([])
            );
            session.close();
        }
        for name in ["heartbeat", "child-heartbeat"] {
            let bytes = fs::read(fixture.root.path().join(name)).unwrap();
            std::thread::sleep(Duration::from_millis(100));
            assert_eq!(fs::read(fixture.root.path().join(name)).unwrap(), bytes);
        }
        assert!(fixture.store().list_certificates().unwrap().is_empty());
        fixture.unchanged_tasks();
    }
}

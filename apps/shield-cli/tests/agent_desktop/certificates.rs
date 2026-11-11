//! 三平台普通管理默认入口验收；只操作本次隔离库和合成证书。
use super::*;

fn private(result: &Value, fixture: &Fixture) {
    let text = result.to_string();
    for secret in [SECRET, fixture.root.path().to_str().unwrap(), "中文🔒Alias"] {
        assert!(!text.contains(secret), "管理回执不得泄漏材料");
    }
}

#[test]
#[ignore = "需真实 JDK、正式适配资源及自有 APK；验证普通证书管理而非显式夹具建立"]
fn 真实默认创建导入校验和删除在同一会话完成() {
    let fixture = Fixture::new();
    let paths = ApplicationPaths::isolated(&fixture.root.path().join("home")).unwrap();
    let store =
        CertificateStoreState::initialize(paths.certificate_database(), paths.keystore_dir())
            .unwrap();
    let original = serde_json::to_value(store.list_certificates().unwrap()).unwrap();
    let mut command = fixture.mcp(false, 120);
    command.env("MOCIKA_SHIELD_KS_PASS", SECRET);
    let mut session = client::Session::start(command);
    let arguments = json!({"name":"默认创建测试","file_name":"中文🔒证书.p12","alias":"中文🔒Alias","dname":"CN=默认创建测试,O=隔离组织,C=CN","keystore_type":"pkcs12","set_default":false});
    let created = session.call(2, "shield_create_certificate", arguments.clone());
    assert_eq!(created["status"], "ready", "{created}");
    assert_eq!(created["change_state"], "confirmed");
    private(&created, &fixture);
    let first = created["data"]["certificate"]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let record = store.get_certificate(&first).unwrap().unwrap();
    assert_eq!(record.key_alias, "中文🔒alias");
    assert_eq!(record.keystore_password, SECRET);
    let material = fs::read(&record.keystore_path).unwrap();
    let verified = session.call(
        3,
        "shield_verify_certificate",
        json!({"certificate_id":first}),
    );
    assert_eq!(
        verified["data"]["certificate"]["verification_status"],
        "success"
    );
    private(&verified, &fixture);
    fs::write(&record.keystore_path, b"invalid-fixture-keystore").unwrap();
    let failed = session.call(
        4,
        "shield_verify_certificate",
        json!({"certificate_id":first}),
    );
    assert_eq!(failed["status"], "blocked");
    assert_eq!(failed["change_state"], "confirmed");
    assert_eq!(failed["session_execution_blocked"], false);
    private(&failed, &fixture);
    fs::write(&record.keystore_path, &material).unwrap();
    let source = fixture.root.path().join("input/中文🔒导入证书.p12");
    fs::write(&source, &material).unwrap();
    let imported = session.call(
        5,
        "shield_import_certificate",
        json!({"name":"默认导入测试","keystore":"中文🔒导入证书.p12","keystore_type":"pkcs12"}),
    );
    assert_eq!(imported["status"], "ready", "{imported}");
    private(&imported, &fixture);
    let second = imported["data"]["certificate"]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let imported_record = store.get_certificate(&second).unwrap().unwrap();
    assert_eq!(
        fs::canonicalize(&imported_record.keystore_path).unwrap(),
        fs::canonicalize(&source).unwrap()
    );
    assert_eq!(imported_record.key_alias, record.key_alias);
    let duplicate = session.call(6, "shield_create_certificate", arguments);
    assert_eq!(duplicate["status"], "ready", "{duplicate}");
    let third = duplicate["data"]["certificate"]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_ne!(
        store
            .get_certificate(&third)
            .unwrap()
            .unwrap()
            .keystore_path,
        record.keystore_path
    );
    let third_path = store
        .get_certificate(&third)
        .unwrap()
        .unwrap()
        .keystore_path;
    for id in [&first, &second, &third] {
        let removed = session.call(7, "shield_remove_certificate", json!({"certificate_id":id}));
        assert_eq!(removed["status"], "ready");
        assert_eq!(removed["data"]["keystore_file"], "retained");
    }
    assert_eq!(
        serde_json::to_value(store.list_certificates().unwrap()).unwrap(),
        original
    );
    assert_eq!(fs::read(&record.keystore_path).unwrap(), material);
    assert_eq!(fs::read(&source).unwrap(), material);
    assert!(Path::new(&third_path).is_file());
    let listing = session.call(8, "shield_certificates", json!({}));
    assert_eq!(listing["status"], "ready");
    session.close();
    fixture.check_unchanged();
    println!("默认管理闭环：创建、实际 Alias、校验成功／损坏失败、恢复后导入、同名无覆盖、删除留文件、原记录与默认项不变均通过");
}

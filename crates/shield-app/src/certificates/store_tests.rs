use super::*;
use tempfile::TempDir;

#[test]
fn 默认身份解析立即固化且明确编号失效不回退() {
    let (_temp, store) = create_store();
    let mut draft = input(None);
    draft.key_password.clear();
    let first = store
        .save_certificate(&draft, "/first.p12", "first", None)
        .unwrap();
    store.set_default_certificate(Some(&first.id)).unwrap();
    let snapshot = super::super::resolve_signing_certificate(
        &store,
        super::super::CertificateSelection::Default,
    )
    .unwrap();
    assert_eq!(snapshot.key_password, snapshot.keystore_password);
    assert!(
        store
            .get_certificate(&first.id)
            .unwrap()
            .unwrap()
            .key_password
            .is_empty(),
        "回退只作用本次快照，不重写密文"
    );
    let second = store
        .save_certificate(&input(None), "/second.p12", "second", None)
        .unwrap();
    store.set_default_certificate(Some(&second.id)).unwrap();
    assert_eq!(snapshot.id, first.id);
    assert_eq!(
        super::super::resolve_signing_certificate(
            &store,
            super::super::CertificateSelection::Default
        )
        .unwrap()
        .id,
        second.id
    );
    assert!(super::super::resolve_signing_certificate(
        &store,
        super::super::CertificateSelection::Id("missing")
    )
    .is_err());
    store.set_default_certificate(None).unwrap();
    assert!(super::super::resolve_signing_certificate(
        &store,
        super::super::CertificateSelection::Default
    )
    .is_err());
}

#[test]
fn 只读打开不初始化目录或数据库且不能写入() {
    let temp = tempfile::tempdir().unwrap();
    let database = temp.path().join("不存在/shield.db");
    let keystores = temp.path().join("不存在/keystores");
    assert!(CertificateStoreState::open_read_only(database.clone(), keystores).is_err());
    assert!(!database.parent().unwrap().exists());
    let (temp, store) = create_store();
    let created = store
        .save_certificate(&input(None), "/fixture.p12", "release", None)
        .unwrap();
    let raw = fs::read(temp.path().join("shield.db")).unwrap();
    let reader = CertificateStoreState::open_read_only(
        temp.path().join("shield.db"),
        temp.path().join("keystores"),
    )
    .unwrap();
    assert_eq!(reader.list_certificates().unwrap().len(), 1);
    assert!(reader.set_default_certificate(Some(&created.id)).is_err());
    assert_eq!(fs::read(temp.path().join("shield.db")).unwrap(), raw);
    assert!(!temp.path().join("shield.db-journal").exists());
    fs::remove_file(temp.path().join("shield.db")).unwrap();
    assert!(reader.list_certificates().is_err());
    assert!(store.list_certificates().is_err());
    assert!(store.set_default_certificate(None).is_err());
    assert!(
        !temp.path().join("shield.db").exists(),
        "已加载库删除后不得自动新建空库"
    );
}

#[test]
fn 字段偏好更新保留其他修改且坏密文仍可查询编辑删除记录() {
    let (temp, first) = create_store();
    let second = CertificateStoreState::initialize(
        temp.path().join("shield.db"),
        temp.path().join("keystores"),
    )
    .unwrap();
    let created = first
        .save_certificate(&input(None), "/fixture.p12", "release", None)
        .unwrap();
    first
        .patch_certificate_preferences(
            &created.id,
            CertificatePreferencesPatch {
                name: Some("甲入口的名称".into()),
                ..Default::default()
            },
            false,
        )
        .unwrap();
    second
        .patch_certificate_preferences(
            &created.id,
            CertificatePreferencesPatch {
                note: Some("乙入口的备注".into()),
                sign_v3: Some(false),
                ..Default::default()
            },
            true,
        )
        .unwrap();
    let record = first.get_certificate(&created.id).unwrap().unwrap();
    assert_eq!(record.name, "甲入口的名称");
    assert_eq!(record.note, "乙入口的备注");
    assert!(!record.sign_v3);
    assert!(record.is_default);
    assert_eq!(record.keystore_path, "/fixture.p12");
    assert_eq!(record.keystore_password, "old-store-pass");
    first
        .open()
        .unwrap()
        .execute(
            "UPDATE certificates SET keystore_password='broken' WHERE id=?1",
            [&created.id],
        )
        .unwrap();
    assert!(first.get_certificate(&created.id).is_err());
    assert_eq!(
        second
            .patch_certificate_preferences(
                &created.id,
                CertificatePreferencesPatch {
                    note: Some("仍可修正备注".into()),
                    ..Default::default()
                },
                false
            )
            .unwrap()
            .note,
        "仍可修正备注"
    );
    second.delete_certificate(&created.id, false).unwrap();
    assert!(first.list_certificates().unwrap().is_empty());
}

#[test]
fn 两个独立实例共用证书默认项且公开摘要没有密码字段() {
    let (temp, first) = create_store();
    let second = CertificateStoreState::initialize(
        temp.path().join("shield.db"),
        temp.path().join("keystores"),
    )
    .unwrap();
    let created = first
        .save_certificate(&input(None), "/old/release.p12", "中文Release", None)
        .unwrap();
    let summary = second.list_certificates().unwrap().pop().unwrap();
    assert_eq!(summary.id, created.id);
    assert_eq!(summary.key_alias, "中文Release");
    let encoded = serde_json::to_value(&summary).unwrap();
    assert!(encoded.get("keystore_password").is_none());
    assert!(encoded.get("key_password").is_none());
    assert!(!encoded.to_string().contains("old-store-pass"));
    assert!(!encoded.to_string().contains("old-key-pass"));
    second.set_default_certificate(Some(&created.id)).unwrap();
    assert!(first.list_certificates().unwrap()[0].is_default);
    let mut edit = input(Some(&created.id));
    edit.note = "另一入口保存的备注".into();
    second.update_certificate_preferences(&edit).unwrap();
    assert_eq!(
        first.list_certificates().unwrap()[0].note,
        "另一入口保存的备注"
    );
    first.set_default_certificate(None).unwrap();
    assert!(!second.list_certificates().unwrap()[0].is_default);
}

#[test]
fn 旧版密文与原路径上下文可以重开而新目录不能冒用() {
    use chacha20poly1305::{
        aead::{Aead, KeyInit},
        ChaCha20Poly1305, Nonce,
    };
    use sha2::{Digest, Sha256};
    let (temp, first) = create_store();
    let created = first
        .save_certificate(&input(None), "/old/release.p12", "release", None)
        .unwrap();
    // 冻结迁移前的编码与派生步骤，不能通过调用新实现来制造“旧”夹具。
    let mut legacy = Sha256::new();
    legacy.update(b"mocika-shield-gui-cert-store-v1");
    legacy.update(temp.path().join("shield.db").to_string_lossy().as_bytes());
    legacy.update(temp.path().join("keystores").to_string_lossy().as_bytes());
    if let Some(home) = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")) {
        legacy.update(home.to_string_lossy().as_bytes());
    }
    if let Some(user) = std::env::var_os("USER").or_else(|| std::env::var_os("USERNAME")) {
        legacy.update(user.to_string_lossy().as_bytes());
    }
    let key: [u8; 32] = legacy.finalize().into();
    let cipher = ChaCha20Poly1305::new_from_slice(&key).unwrap();
    let encrypted = cipher
        .encrypt(
            Nonce::from_slice(&[3; 12]),
            b"legacy-test-only-password".as_ref(),
        )
        .unwrap();
    let encoded = format!(
        "enc:v1:{}:{}",
        "03".repeat(12),
        encrypted
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    );
    first
        .open()
        .unwrap()
        .execute(
            "UPDATE certificates SET keystore_password=?1,key_password=?1 WHERE id=?2",
            params![encoded, created.id],
        )
        .unwrap();
    let reopened = CertificateStoreState::initialize(
        temp.path().join("shield.db"),
        temp.path().join("keystores"),
    )
    .unwrap();
    let loaded = reopened.get_certificate(&created.id).unwrap().unwrap();
    assert!(loaded.keystore_password == "legacy-test-only-password");
    assert!(loaded.key_password == "legacy-test-only-password");
    let raw: String = first
        .open()
        .unwrap()
        .query_row(
            "SELECT keystore_password FROM certificates WHERE id=?1",
            [&created.id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(raw, encoded, "提取共享层不重写旧密文");
    let other = temp.path().join("隔离 中文目录");
    fs::create_dir(&other).unwrap();
    fs::copy(temp.path().join("shield.db"), other.join("shield.db")).unwrap();
    let moved = CertificateStoreState::initialize(other.join("shield.db"), other.join("keystores"))
        .unwrap();
    assert!(
        moved.get_certificate(&created.id).is_err(),
        "不能将复制到新目录误认为密码迁移成功"
    );
}

#[test]
fn 不接受明文且坏密文不阻止安全摘要查询() {
    let (_temp, store) = create_store();
    let created = store
        .save_certificate(&input(None), "/old/release.p12", "release", None)
        .unwrap();
    store.open().unwrap().execute("UPDATE certificates SET keystore_password='legacy-plaintext-not-supported' WHERE id=?1", [&created.id]).unwrap();
    assert!(store.get_certificate(&created.id).is_err());
    assert_eq!(store.list_certificates().unwrap()[0].id, created.id);
}

#[test]
fn 删除外部记录只清默认项不删除原证书且保存不能覆盖材料() {
    let (temp, store) = create_store();
    let external = temp.path().join("用户外部证书.p12");
    fs::write(&external, b"fixture-not-keystore").unwrap();
    let mut draft = input(None);
    draft.source_type = "external".into();
    draft.set_as_default = true;
    let created = store
        .save_certificate(&draft, external.to_str().unwrap(), "release", None)
        .unwrap();
    draft.id = Some(created.id.clone());
    assert!(store
        .save_certificate(&draft, "/changed/material.p12", "changed", None)
        .is_err());
    store.delete_certificate(&created.id, true).unwrap();
    assert!(store.list_certificates().unwrap().is_empty());
    assert!(get_default_certificate_id(&store.open().unwrap())
        .unwrap()
        .is_none());
    assert_eq!(fs::read(&external).unwrap(), b"fixture-not-keystore");
}

#[cfg(unix)]
#[test]
fn 初始化仅收紧隔离数据目录与数据库权限() {
    use std::os::unix::fs::PermissionsExt;
    let temp = tempfile::tempdir().unwrap();
    let data = temp.path().join("应用数据");
    let _store =
        CertificateStoreState::initialize(data.join("shield.db"), data.join("keystores")).unwrap();
    for (path, expected) in [
        (data.clone(), 0o700),
        (data.join("keystores"), 0o700),
        (data.join("shield.db"), 0o600),
    ] {
        assert_eq!(
            fs::metadata(path).unwrap().permissions().mode() & 0o777,
            expected
        );
    }
}

fn create_store() -> (TempDir, CertificateStoreState) {
    let temp = TempDir::new().expect("创建临时目录失败");
    let store = CertificateStoreState::initialize(
        temp.path().join("shield.db"),
        temp.path().join("keystores"),
    )
    .expect("初始化测试数据库失败");
    (temp, store)
}

fn input(id: Option<&str>) -> CertificateUpsertInput {
    CertificateUpsertInput {
        id: id.map(|value| value.to_string()),
        name: "发布证书".to_string(),
        source_type: "managed".to_string(),
        keystore_path: "/old/release.p12".to_string(),
        keystore_password: "old-store-pass".to_string(),
        key_alias: "release".to_string(),
        key_password: "old-key-pass".to_string(),
        ks_type: Some("PKCS12".to_string()),
        sign_v1: true,
        sign_v2: true,
        sign_v3: true,
        sign_v4: false,
        auto_sign_enabled: true,
        note: "初始备注".to_string(),
        set_as_default: false,
        copy_keystore_to_managed: false,
        managed_file_name: None,
    }
}

#[test]
fn 更新证书偏好时不覆盖证书材料字段() {
    let (_temp, store) = create_store();
    let created = store
        .save_certificate(
            &input(None),
            "/old/release.p12",
            "release",
            Some(("success", Some("校验通过"), 100)),
        )
        .expect("保存初始证书失败");

    let mut edit = input(Some(&created.id));
    edit.name = "生产证书".to_string();
    edit.source_type = "external".to_string();
    edit.keystore_path = "/new/changed.jks".to_string();
    edit.keystore_password = "new-store-pass".to_string();
    edit.key_alias = "changed".to_string();
    edit.key_password = "new-key-pass".to_string();
    edit.ks_type = Some("JKS".to_string());
    edit.sign_v1 = false;
    edit.sign_v2 = true;
    edit.sign_v3 = false;
    edit.sign_v4 = true;
    edit.auto_sign_enabled = false;
    edit.note = "仅更新偏好".to_string();
    edit.set_as_default = true;

    let updated = store
        .update_certificate_preferences(&edit)
        .expect("更新证书偏好失败");

    assert_eq!(updated.name, "生产证书");
    assert!(!updated.sign_v1);
    assert!(updated.sign_v2);
    assert!(!updated.sign_v3);
    assert!(updated.sign_v4);
    assert!(!updated.auto_sign_enabled);
    assert_eq!(updated.note, "仅更新偏好");
    assert!(updated.is_default);

    assert_eq!(updated.source_type, "managed");
    assert_eq!(updated.keystore_path, "/old/release.p12");
    assert_eq!(updated.keystore_password, "old-store-pass");
    assert_eq!(updated.key_alias, "release");
    assert_eq!(updated.key_password, "old-key-pass");
    assert_eq!(updated.ks_type, "PKCS12");
    assert_eq!(updated.last_verified_at, Some(100));
    assert_eq!(updated.last_verify_status, "success");
    assert_eq!(updated.last_verify_message.as_deref(), Some("校验通过"));
}

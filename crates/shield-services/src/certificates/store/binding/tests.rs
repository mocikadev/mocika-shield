use super::*;
use crate::certificates::{CertificatePreferencesPatch, CertificateUpsertInput};
use rusqlite::params;

const SECRET: &str = "binding-synthetic-password";
fn fixture() -> (tempfile::TempDir, CertificateStoreState, String) {
    let temp = tempfile::tempdir().unwrap();
    let store = CertificateStoreState::initialize(
        temp.path().join("shield.db"),
        temp.path().join("keystores"),
    )
    .unwrap();
    let file = temp.path().join("测试证书.p12");
    std::fs::write(&file, b"fixture-not-keystore").unwrap();
    let record = store
        .save_certificate(&draft(), file.to_str().unwrap(), "中文Release", None)
        .unwrap();
    (temp, store, record.id)
}
fn draft() -> CertificateUpsertInput {
    CertificateUpsertInput {
        id: None,
        name: "测试证书".into(),
        source_type: "external".into(),
        keystore_path: String::new(),
        keystore_password: SECRET.into(),
        key_alias: "中文Release".into(),
        key_password: String::new(),
        ks_type: Some("PKCS12".into()),
        sign_v1: true,
        sign_v2: true,
        sign_v3: true,
        sign_v4: false,
        auto_sign_enabled: true,
        note: "本地备注".into(),
        set_as_default: true,
        copy_keystore_to_managed: false,
        managed_file_name: None,
    }
}

#[test]
fn 绑定可安全往返而材料和默认项来自同一只读快照() {
    let (temp, store, id) = fixture();
    let before = std::fs::read(temp.path().join("shield.db")).unwrap();
    let reader = CertificateStoreState::open_read_only(
        temp.path().join("shield.db"),
        temp.path().join("keystores"),
    )
    .unwrap();
    let identity = reader
        .capture_signing_identity(CertificateSelection::Default)
        .ok()
        .unwrap();
    assert_eq!(identity.certificate().id, id);
    assert!(identity.certificate().is_default);
    assert_eq!(identity.certificate().key_password, SECRET);
    let encoded = serde_json::to_value(identity.binding()).unwrap();
    assert_eq!(encoded.as_object().unwrap().len(), 3);
    assert_eq!(encoded["schema_version"], 1);
    assert_eq!(encoded["id"], id);
    assert_eq!(encoded["state_sha256"].as_array().unwrap().len(), 32);
    let text = encoded.to_string();
    for forbidden in [SECRET, "测试证书.p12", "中文Release", "本地备注", "enc:v1:"] {
        assert!(!text.contains(forbidden));
    }
    let restored: CertificateBinding = serde_json::from_value(encoded).unwrap();
    assert!(reader.check_certificate_binding(&restored).is_ok());
    let encrypted = super::super::read_encrypted_certificate(&store.open().unwrap(), &id)
        .unwrap()
        .unwrap();
    assert!(encrypted.keystore_password.starts_with("enc:v1:"));
    assert!(store
        .get_certificate(&id)
        .unwrap()
        .unwrap()
        .key_password
        .is_empty());
    assert_eq!(
        std::fs::read(temp.path().join("shield.db")).unwrap(),
        before
    );
}

#[test]
fn 名称备注校验默认和自动签名偏好不作废已明确的任务() {
    let (_temp, store, id) = fixture();
    let identity = store
        .capture_signing_identity(CertificateSelection::Default)
        .ok()
        .unwrap();
    store
        .patch_certificate_preferences(
            &id,
            CertificatePreferencesPatch {
                name: Some("改名".into()),
                note: Some("新备注".into()),
                auto_sign_enabled: Some(false),
                ..Default::default()
            },
            false,
        )
        .unwrap();
    store
        .update_verify_status(&id, "success", Some("新的校验说明"), None)
        .unwrap();
    let another = store
        .save_certificate(&draft(), "/unrelated.p12", "another", None)
        .unwrap();
    assert_ne!(another.id, id);
    assert!(store.check_certificate_binding(identity.binding()).is_ok());
    let current = store
        .capture_signing_identity(CertificateSelection::Id(&id))
        .ok()
        .unwrap();
    assert!(current.binding() == identity.binding());
    assert!(!current.certificate().is_default);
    assert!(identity.certificate().is_default);
    assert_eq!(identity.certificate().name, "测试证书");
}

#[test]
fn 签名版本任一变化都要求重新规划() {
    let (_temp, store, id) = fixture();
    let identity = store
        .capture_signing_identity(CertificateSelection::Id(&id))
        .ok()
        .unwrap();
    for (column, old, new) in [
        ("sign_v1", 1, 0),
        ("sign_v2", 1, 0),
        ("sign_v3", 1, 0),
        ("sign_v4", 0, 1),
    ] {
        // 列名全部来自固定测试白名单，不接受调用者输入。
        store
            .open()
            .unwrap()
            .execute(
                &format!("UPDATE certificates SET {column}=?1 WHERE id=?2"),
                params![new, id],
            )
            .unwrap();
        assert_eq!(
            store.check_certificate_binding(identity.binding()).err(),
            Some(CertificateBindingError::RecordChanged)
        );
        store
            .open()
            .unwrap()
            .execute(
                &format!("UPDATE certificates SET {column}=?1 WHERE id=?2"),
                params![old, id],
            )
            .unwrap();
        assert!(store.check_certificate_binding(identity.binding()).is_ok());
    }
}

#[test]
fn 证书材料字段和密文变化要求重新规划且不哈希明文口令() {
    let (_temp, store, id) = fixture();
    let identity = store
        .capture_signing_identity(CertificateSelection::Id(&id))
        .ok()
        .unwrap();
    let raw = super::super::read_encrypted_certificate(&store.open().unwrap(), &id)
        .unwrap()
        .unwrap();
    let secret =
        super::super::encrypt_secret(&store.crypto_key, "other-synthetic-password").unwrap();
    for (column, before, after) in [
        ("keystore_path", raw.keystore_path.as_str(), "/another.p12"),
        ("key_alias", raw.key_alias.as_str(), "another-alias"),
        ("ks_type", raw.ks_type.as_str(), "JKS"),
        (
            "keystore_password",
            raw.keystore_password.as_str(),
            secret.as_str(),
        ),
        ("key_password", raw.key_password.as_str(), secret.as_str()),
    ] {
        store
            .open()
            .unwrap()
            .execute(
                &format!("UPDATE certificates SET {column}=?1 WHERE id=?2"),
                params![after, id],
            )
            .unwrap();
        assert_eq!(
            store.check_certificate_binding(identity.binding()).err(),
            Some(CertificateBindingError::RecordChanged)
        );
        store
            .open()
            .unwrap()
            .execute(
                &format!("UPDATE certificates SET {column}=?1 WHERE id=?2"),
                params![before, id],
            )
            .unwrap();
    }
    store
        .open()
        .unwrap()
        .execute(
            "UPDATE certificates SET keystore_password=?1 WHERE id=?2",
            params![SECRET, id],
        )
        .unwrap();
    assert_eq!(
        store
            .capture_signing_identity(CertificateSelection::Id(&id))
            .err(),
        Some(CertificateBindingError::CredentialsInvalid)
    );
    assert_eq!(
        store.check_certificate_binding(identity.binding()).err(),
        Some(CertificateBindingError::RecordChanged)
    );
}

#[test]
fn 删除原记录后不回退新默认项() {
    let (_temp, store, id) = fixture();
    let identity = store
        .capture_signing_identity(CertificateSelection::Default)
        .ok()
        .unwrap();
    store
        .save_certificate(&draft(), "/other.p12", "other", None)
        .unwrap();
    store.delete_certificate(&id, false).unwrap();
    assert_eq!(
        store.check_certificate_binding(identity.binding()).err(),
        Some(CertificateBindingError::NotFound)
    );
    assert_eq!(
        store
            .capture_signing_identity(CertificateSelection::Id(&id))
            .err(),
        Some(CertificateBindingError::NotFound)
    );
}

#[test]
fn 绑定上下文不同不能冒用复制的数据库() {
    let (temp, store, id) = fixture();
    let identity = store
        .capture_signing_identity(CertificateSelection::Id(&id))
        .ok()
        .unwrap();
    let other = tempfile::tempdir().unwrap();
    std::fs::copy(
        temp.path().join("shield.db"),
        other.path().join("shield.db"),
    )
    .unwrap();
    let moved = CertificateStoreState::open_read_only(
        other.path().join("shield.db"),
        other.path().join("keystores"),
    )
    .unwrap();
    assert_eq!(
        moved.check_certificate_binding(identity.binding()).err(),
        Some(CertificateBindingError::RecordChanged)
    );
    assert_eq!(
        moved
            .capture_signing_identity(CertificateSelection::Id(&id))
            .err(),
        Some(CertificateBindingError::CredentialsInvalid)
    );
}

#[test]
fn 未知绑定字段版本或篡改摘要不会被接受() {
    let (_temp, store, id) = fixture();
    let identity = store
        .capture_signing_identity(CertificateSelection::Id(&id))
        .ok()
        .unwrap();
    let mut encoded = serde_json::to_value(identity.binding()).unwrap();
    encoded["password"] = serde_json::json!(SECRET);
    assert!(serde_json::from_value::<CertificateBinding>(encoded).is_err());
    let mut changed = identity.binding().clone();
    changed.state_sha256[0] ^= 1;
    assert_eq!(
        store.check_certificate_binding(&changed).err(),
        Some(CertificateBindingError::RecordChanged)
    );
    changed.schema_version = 2;
    assert_eq!(
        store.check_certificate_binding(&changed).err(),
        Some(CertificateBindingError::UnsupportedVersion)
    );
}

#[test]
fn 记录绑定不冒充外部证书文件摘要() {
    let (temp, store, id) = fixture();
    let identity = store
        .capture_signing_identity(CertificateSelection::Id(&id))
        .ok()
        .unwrap();
    std::fs::write(temp.path().join("测试证书.p12"), b"external-file-changed").unwrap();
    assert!(
        store.check_certificate_binding(identity.binding()).is_ok(),
        "文件字节仍须由执行器的 FileSnapshot 单独核对"
    );
}

#[test]
fn 提交前绑定复核不读取无关备注和校验时间() {
    let (_temp, store, id) = fixture();
    let identity = store
        .capture_signing_identity(CertificateSelection::Id(&id))
        .ok()
        .unwrap();
    // 模拟无关元数据损坏；已明确的材料绑定仍不应改为整行解码或整库摘要。
    store
        .open()
        .unwrap()
        .execute(
            "UPDATE certificates SET last_verified_at='invalid-metadata' WHERE id=?1",
            [&id],
        )
        .unwrap();
    assert!(store.check_certificate_binding(identity.binding()).is_ok());
}

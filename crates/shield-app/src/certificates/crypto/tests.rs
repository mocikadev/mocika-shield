use super::*;

fn keys(user: Option<&str>, account: Option<&str>) -> SecretKeys {
    keys_for(
        Path::new("/本地/shield.db"),
        Path::new("/本地/keystores"),
        Some(OsStr::new("/账户")),
        user.map(OsStr::new),
        account.map(OsStr::new),
    )
}
fn record(store: String, key: String) -> CertificateRecord {
    CertificateRecord {
        id: "fixture".into(),
        name: String::new(),
        source_type: "external".into(),
        keystore_path: "fixture.p12".into(),
        keystore_password: store,
        key_alias: "fixture".into(),
        key_password: key,
        ks_type: "PKCS12".into(),
        sign_v1: true,
        sign_v2: true,
        sign_v3: true,
        sign_v4: false,
        auto_sign_enabled: true,
        note: String::new(),
        is_default: true,
        created_at: 0,
        updated_at: 0,
        last_verified_at: None,
        last_verify_status: "unknown".into(),
        last_verify_message: None,
    }
}
fn encrypted(keys: &SecretKeys) -> CertificateRecord {
    record(
        encrypt_secret(keys, "合成证书口令").unwrap(),
        encrypt_secret(keys, "合成密钥口令").unwrap(),
    )
}
#[test]
fn 用户名有无交叉读取且两个字段都成功() {
    let named = keys(Some("开发者"), None);
    let absent = keys(None, Some("开发者"));
    for (writer, reader) in [(&named, &absent), (&absent, &named)] {
        let mut item = encrypted(writer);
        decrypt_record_secrets(reader, &mut item).unwrap();
        assert!(item.keystore_password == "合成证书口令");
        assert!(item.key_password == "合成密钥口令");
    }
}
#[test]
fn 写入与冻结的旧上下文完全兼容且不依赖系统候选() {
    for user in [None, Some("开发者"), Some("")] {
        let keys = keys(user, Some("不应加入写入的系统名"));
        let mut legacy = Sha256::new();
        legacy.update(b"mocika-shield-gui-cert-store-v1");
        legacy.update("/本地/shield.db".as_bytes());
        legacy.update("/本地/keystores".as_bytes());
        legacy.update("/账户".as_bytes());
        if let Some(user) = user {
            legacy.update(user.as_bytes());
        }
        let legacy: [u8; 32] = legacy.finalize().into();
        assert!(
            decrypt_secret(&legacy, &encrypt_secret(&keys, "旧版也可读取").unwrap()).unwrap()
                == "旧版也可读取"
        );
    }
}
#[test]
fn 两个密码不能分别使用不同候选且失败不留下半明文() {
    let named = keys(Some("开发者"), None);
    let absent = keys(None, Some("开发者"));
    let store = encrypt_secret(&named, "合成证书口令").unwrap();
    let key = encrypt_secret(&absent, "合成密钥口令").unwrap();
    let mut item = record(store.clone(), key.clone());
    assert!(decrypt_record_secrets(&absent, &mut item).is_err());
    assert_eq!(item.keystore_password, store);
    assert_eq!(item.key_password, key);
}
#[test]
fn 不猜其他账户也不忽略路径或主目录() {
    let original = keys(Some("开发者"), None);
    for reader in [
        keys(None, Some("其他账户")),
        keys(Some("其他账户"), Some("开发者")),
        keys_for(
            Path::new("/其他/shield.db"),
            Path::new("/本地/keystores"),
            Some(OsStr::new("/账户")),
            None,
            Some(OsStr::new("开发者")),
        ),
        keys_for(
            Path::new("/本地/shield.db"),
            Path::new("/本地/keystores"),
            Some(OsStr::new("/其他账户")),
            None,
            Some(OsStr::new("开发者")),
        ),
    ] {
        assert!(decrypt_record_secrets(&reader, &mut encrypted(&original)).is_err());
    }
}
#[test]
fn 无系统账户时只保留原写入候选并拒绝明文和坏格式() {
    let keys = keys(None, None);
    assert!(keys.fallback.is_none());
    for bad in ["明文不能接收", "enc:v1:00:00", "enc:v1:不是hex:00"] {
        let mut item = record(bad.into(), bad.into());
        assert!(decrypt_record_secrets(&keys, &mut item).is_err());
        assert_eq!(item.keystore_password, bad);
    }
}
#[test]
fn 空用户名保持旧语义且不转而查询系统用户() {
    let keys = keys(Some(""), Some("开发者"));
    assert!(keys.fallback.is_none());
    let mut item = encrypted(&keys);
    decrypt_record_secrets(&self::keys(None, None), &mut item).unwrap();
}

//! 显式运行的真实 JDK 回归；所有证书、数据库、签名输出仅在临时目录中生成。
use super::*;
use crate::cert_service::{create_managed_certificate, save_certificate_profile};
use crate::signing::query_keystore_aliases;
use shield_core::{
    extract_apk_cert_fingerprint, extract_keystore_cert_fingerprint, sign_apk, KeystoreType,
    SignOptions, SigningVersions,
};

#[test]
#[ignore = "需要真实 JDK、SHIELD_TEST_APK 和 SHIELD_TEST_APKSIGNER，手动执行"]
fn 中文证书真实导入保存重开及签名() {
    let apk = PathBuf::from(std::env::var_os("SHIELD_TEST_APK").expect("指定自有测试 APK"));
    let signer = PathBuf::from(std::env::var_os("SHIELD_TEST_APKSIGNER").expect("指定签名工具"));
    assert!(apk.is_file() && signer.is_file());
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("中文证书 空格");
    fs::create_dir_all(&dir).unwrap();
    let db = dir.join("shield.db");
    let store = CertificateStoreState::new(db.clone(), dir.clone());
    initialize_schema(&store.open().unwrap()).unwrap();
    // 仅用于测试的公开口令，不使用也不输出用户秘密。
    let password = "test-only-123456";
    for kind in ["JKS", "PKCS12"] {
        for alias in ["release", "中文签名", "中文ReleasePOS"] {
            let expected = alias.to_lowercase();
            let created = create_managed_certificate(
                &store,
                CreateManagedCertificateInput {
                    name: format!("{kind}-{alias}"),
                    file_name: format!("{kind}-{alias}"),
                    key_alias: alias.into(),
                    keystore_password: password.into(),
                    key_password: password.into(),
                    ks_type: Some(kind.into()),
                    sign_v1: true,
                    sign_v2: true,
                    sign_v3: true,
                    sign_v4: false,
                    auto_sign_enabled: true,
                    note: "临时回归".into(),
                    set_as_default: false,
                    dname: "CN=中文测试,O=Mocika,C=CN".into(),
                    validity_days: 7,
                    key_size: 2048,
                },
            )
            .unwrap();
            let original = fs::read(&created.keystore_path).unwrap();
            assert_eq!(
                query_keystore_aliases(
                    created.keystore_path.clone(),
                    password.into(),
                    Some(kind.into())
                )
                .unwrap(),
                vec![expected.clone()]
            );
            let imported = save_certificate_profile(
                &store,
                CertificateUpsertInput {
                    id: None,
                    name: "导入中文证书".into(),
                    source_type: "external".into(),
                    keystore_path: created.keystore_path.clone(),
                    keystore_password: password.into(),
                    key_alias: alias.into(),
                    key_password: password.into(),
                    ks_type: Some(kind.into()),
                    sign_v1: true,
                    sign_v2: true,
                    sign_v3: true,
                    sign_v4: false,
                    auto_sign_enabled: true,
                    note: String::new(),
                    set_as_default: true,
                    copy_keystore_to_managed: false,
                    managed_file_name: None,
                },
            )
            .unwrap();
            let reopened = CertificateStoreState::new(db.clone(), dir.clone());
            let loaded = reopened.get_certificate(&imported.id).unwrap().unwrap();
            assert_eq!(loaded.key_alias, expected);
            assert!(loaded.keystore_password == password && loaded.key_password == password);
            let output = dir.join(format!("签名结果-{kind}-{alias}.apk"));
            sign_apk(&SignOptions {
                apk_path: apk.clone(),
                output_path: Some(output.clone()),
                keystore_path: PathBuf::from(&loaded.keystore_path),
                key_alias: loaded.key_alias.clone(),
                keystore_password: loaded.keystore_password,
                key_password: loaded.key_password,
                apksigner_path: Some(signer.clone()),
                keystore_type: KeystoreType::parse(kind),
                signing_versions: SigningVersions::default(),
            })
            .unwrap();
            let ks_fp = extract_keystore_cert_fingerprint(
                Path::new(&loaded.keystore_path),
                &loaded.key_alias,
                password,
                Some(kind),
            )
            .unwrap();
            let apk_fp = extract_apk_cert_fingerprint(&output, Some(&signer)).unwrap();
            assert_eq!(ks_fp, apk_fp);
            assert_eq!(fs::read(&created.keystore_path).unwrap(), original);
            eprintln!("已验证 {kind} / {alias}：创建、识别、导入、重开数据库、签名及指纹一致");
        }
    }
}

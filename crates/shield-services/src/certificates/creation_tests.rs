//! 仅使用本次隔离资料库，验证创建后的保存边界与默认项；不访问真实资料。
use super::*;
use shield_core::{
    utils::{find_java, find_keytool},
    JavaToolContext,
};
use std::{
    fs,
    path::{Path, PathBuf},
};

fn work(parent: &Path, name: &str) -> PathBuf {
    let path = parent.join(name);
    #[cfg(windows)]
    shield_core::windows_private::create_directory(&path).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        fs::DirBuilder::new().mode(0o700).create(&path).unwrap();
    }
    path
}
fn input(name: &str, default: bool) -> CreateManagedCertificateInput {
    CreateManagedCertificateInput {
        name: name.into(),
        file_name: "release".into(),
        key_alias: "中文Release".into(),
        keystore_password: "isolated-create-store-password".into(),
        key_password: "".into(),
        ks_type: Some("JKS".into()),
        sign_v1: true,
        sign_v2: true,
        sign_v3: true,
        sign_v4: false,
        auto_sign_enabled: true,
        note: "隔离创建测试".into(),
        set_as_default: default,
        dname: "CN=隔离创建测试,O=测试组织,C=CN".into(),
        validity_days: 2,
        key_size: 2048,
    }
}
fn jar() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tools/java-adapter/build/java-adapter.jar")
}

#[test]
#[ignore = "需正式 JAR 和显式 Java 8／17；只使用隔离资料库"]
fn 显式新建入库并发不覆盖且失败不改变记录或默认项() {
    let root = tempfile::tempdir().unwrap();
    let store = CertificateStoreState::initialize(
        root.path().join("资料/shield.db"),
        root.path().join("资料/keystores"),
    )
    .unwrap();
    let original = store.keystore_dir().join("release.jks");
    fs::write(&original, b"existing-fixture").unwrap();
    let java = find_java().unwrap();
    let keytool = find_keytool().unwrap();
    let context = JavaToolContext::new(&jar(), &work(root.path(), "首次工作 根")).unwrap();
    let first = create_managed_certificate_with_context(
        &store,
        input("首个", true),
        &context,
        &java,
        &keytool,
    )
    .unwrap();
    assert_eq!(first.key_alias, "中文release");
    assert!(first.is_default);
    assert_eq!(first.keystore_password, input("", false).keystore_password);
    let records = std::thread::scope(|scope| {
        let handles: Vec<_> = (0..2)
            .map(|index| {
                let store = &store;
                let java = &java;
                let keytool = &keytool;
                let directory = work(root.path(), &format!("并发-{index}"));
                scope.spawn(move || {
                    let context = JavaToolContext::new(&jar(), &directory).unwrap();
                    create_managed_certificate_with_context(
                        store,
                        input(&format!("并发-{index}"), false),
                        &context,
                        java,
                        keytool,
                    )
                    .unwrap()
                })
            })
            .collect();
        handles
            .into_iter()
            .map(|handle| handle.join().unwrap())
            .collect::<Vec<_>>()
    });
    let mut paths = std::collections::HashSet::new();
    paths.insert(first.keystore_path.clone());
    for record in records {
        assert!(paths.insert(record.keystore_path));
    }
    assert_eq!(fs::read(&original).unwrap(), b"existing-fixture");
    let before = serde_json::to_value(store.list_certificates().unwrap()).unwrap();
    let files = fs::read_dir(store.keystore_dir()).unwrap().count();
    for case in ["subject", "password", "size"] {
        let mut bad = input("拒绝", true);
        match case {
            "subject" => bad.dname = "invalid subject".into(),
            "password" => {
                bad.ks_type = Some("PKCS12".into());
                bad.key_password = "different-key-password".into();
            }
            _ => bad.key_size = 1024,
        }
        assert!(
            create_managed_certificate_with_context(&store, bad, &context, &java, &keytool)
                .is_err()
        );
        assert_eq!(
            serde_json::to_value(store.list_certificates().unwrap()).unwrap(),
            before
        );
        assert_eq!(fs::read_dir(store.keystore_dir()).unwrap().count(), files);
    }
    let contents = fs::read(root.path().join("资料/shield.db")).unwrap();
    let password = input("", false).keystore_password;
    assert!(!contents
        .windows(password.len())
        .any(|part| part == password.as_bytes()));
    assert_eq!(
        store.get_certificate(&first.id).unwrap().unwrap().id,
        first.id
    );
    println!("共享新建回归：实际 Alias 入库、同名并发无覆盖、密码密文、失败不改记录或默认项均通过");
}

#[test]
#[ignore = "需正式 JAR 和显式 JDK；只读隔离数据库模拟提交失败"]
fn 入库失败保留生成文件但不改变数据库或默认项() {
    let root = tempfile::tempdir().unwrap();
    let db = root.path().join("app/shield.db");
    let store =
        CertificateStoreState::initialize(db.clone(), root.path().join("app/keystores")).unwrap();
    let before = fs::read(&db).unwrap();
    let read_only =
        CertificateStoreState::open_read_only(db.clone(), store.keystore_dir().to_path_buf())
            .unwrap();
    let context = JavaToolContext::new(&jar(), &work(root.path(), "只读库工作根")).unwrap();
    let error = create_managed_certificate_with_context(
        &read_only,
        input("提交失败", true),
        &context,
        &find_java().unwrap(),
        &find_keytool().unwrap(),
    )
    .err()
    .unwrap();
    assert!(error.message.contains("保留"));
    assert!(store.list_certificates().unwrap().is_empty());
    assert_eq!(fs::read(&db).unwrap(), before);
    let files: Vec<_> = fs::read_dir(store.keystore_dir())
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    assert_eq!(files.len(), 1);
    assert!(files[0].is_file());
    println!("入库失败回归：生成证书明确保留，数据库、默认项和历史材料未改变");
}

#[cfg(windows)]
#[test]
#[ignore = "需正式适配资源和真实 JDK，验证 Windows 普通新建默认入口的保存失败"]
fn 默认新建入库失败同样保留证书且数据库不变() {
    let root = tempfile::tempdir().unwrap();
    let db = root.path().join("中文资料/shield.db");
    let store =
        CertificateStoreState::initialize(db.clone(), root.path().join("中文资料/keystores"))
            .unwrap();
    let before = fs::read(&db).unwrap();
    let read_only =
        CertificateStoreState::open_read_only(db.clone(), store.keystore_dir().to_path_buf())
            .unwrap();
    let error = create_managed_certificate(&read_only, input("默认提交失败", true))
        .err()
        .unwrap();
    assert!(error.message.contains("保留"));
    assert!(store.list_certificates().unwrap().is_empty());
    assert_eq!(fs::read(&db).unwrap(), before);
    let files: Vec<_> = fs::read_dir(store.keystore_dir())
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    assert_eq!(files.len(), 1);
    assert!(files[0].is_file());
    println!("Windows 默认新建保存失败：生成证书保留，资料库及默认项未改变");
}

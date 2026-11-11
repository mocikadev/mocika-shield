//! 普通证书适配的真实工具回归；全部使用合成材料，不读取用户证书库。
use super::*;
use std::fs;

#[test]
#[ignore = "需构建正式适配资源及真实 Java 8／17，实际生成临时证书"]
fn 普通适配新建与查询保留中文路径别名且错误不覆盖原件() {
    let root = tempfile::tempdir().unwrap();
    let private = root.path().join("中文材料 根");
    #[cfg(windows)]
    crate::windows_private::create_directory(&private).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        fs::DirBuilder::new().mode(0o700).create(&private).unwrap();
    }
    for kind in ["JKS", "PKCS12"] {
        let path = private.join(format!("中文🔒证书-{kind}"));
        let store_password = if kind == "JKS" {
            "中文存储口令123"
        } else {
            "ordinary-test-password"
        };
        let options = CreateKeystoreOptions {
            kind,
            alias: "中文🔒Release",
            keystore_password: store_password,
            key_password: "",
            dname: "CN=中文测试,O=隔离组织,C=CN",
            validity_days: 2,
            key_size: 2048,
        };
        let created = create_keystore_draft(&path, &options).unwrap();
        assert_eq!(created.alias, "中文🔒release");
        assert_eq!(created.fingerprint.len(), 64);
        let before = fs::read(&path).unwrap();
        crate::java_adapter::certificate_session(|context, java| {
            assert_eq!(
                query_keystore_aliases_with_context(
                    context,
                    java,
                    &path,
                    store_password,
                    Some(kind)
                )?,
                vec![created.alias.clone()]
            );
            assert_eq!(
                extract_keystore_cert_fingerprint_with_context(
                    context,
                    java,
                    &path,
                    &created.alias,
                    store_password,
                    Some(kind)
                )?,
                created.fingerprint
            );
            Ok(())
        })
        .unwrap();
        // Windows 必须验证真正默认入口，而不只显式或测试侧选择。
        #[cfg(windows)]
        {
            assert_eq!(
                query_keystore_aliases(&path, store_password, Some(kind)).unwrap(),
                vec![created.alias.clone()]
            );
            assert_eq!(
                extract_keystore_cert_fingerprint(
                    &path,
                    &created.alias,
                    store_password,
                    Some(kind)
                )
                .unwrap(),
                created.fingerprint
            );
            let failure =
                query_keystore_aliases(&path, "incorrect-test-password", Some(kind)).unwrap_err();
            assert_eq!(
                crate::diagnostic::Diagnostic::from_error(&failure).code,
                crate::diagnostic::FailureCode::KeystorePasswordInvalid
            );
            assert!(!failure.to_string().contains(store_password));
        }
        assert!(create_keystore_draft(&path, &options).is_err());
        assert_eq!(fs::read(&path).unwrap(), before);
    }
    println!("普通证书适配：JKS／PKCS12、中文及补充字符路径／Alias、实际 Alias、指纹和目标无覆盖均通过；Windows 另核对默认查询与密码错误分类");
}

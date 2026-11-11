//! 合成资料的真实只读证书回归；原工具作独立摘要对照，每次查询后复核原件未变化。
use super::*;
use crate::keytool::{
    extract_keystore_cert_fingerprint_with_context, query_keystore_aliases_with_context,
};

use super::certificate_fixture::{unchanged, Stores, INITIAL, UNICODE};

#[cfg(windows)]
#[allow(dead_code)]
mod permissions {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/fixtures/windows_permissions.rs"
    ));
}

#[test]
#[ignore = "需正式 JAR 和显式 Java 8／17；仅新建合成证书，不使用真实资料"]
fn 只读查询保留中文全部条目且指纹与独立工具一致() {
    let stores = Stores::new();
    let context = stores.context();
    let bootstrap = stores.root.path().join("bootstrap.jks");
    assert_eq!(
        query_keystore_aliases_with_context(&context, &stores.java, &bootstrap, INITIAL, None)
            .unwrap(),
        ["release"]
    );
    assert_eq!(
        extract_keystore_cert_fingerprint_with_context(
            &context,
            &stores.java,
            &bootstrap,
            "release",
            INITIAL,
            None
        )
        .unwrap(),
        stores.fingerprint
    );
    for (kind, password) in [("JKS", UNICODE), ("PKCS12", INITIAL)] {
        let path = stores.create(kind, "normal", password);
        let hash = Sha256::digest(fs::read(&path).unwrap()).into();
        let mut expected = vec![
            "中文release".to_string(),
            "含,引号\"😀".to_string(),
            "受信 任".to_string(),
        ];
        if kind == "PKCS12" {
            expected.push("secret".into());
        }
        expected.sort();
        let mut aliases = query_keystore_aliases_with_context(
            &context,
            &stores.java,
            &path,
            password,
            Some(kind),
        )
        .unwrap();
        aliases.sort();
        assert_eq!(aliases, expected);
        unchanged(&path, &hash);
        for alias in ["中文Release", "含,引号\"😀", "受信 任"] {
            assert_eq!(
                extract_keystore_cert_fingerprint_with_context(
                    &context,
                    &stores.java,
                    &path,
                    alias,
                    password,
                    Some(kind)
                )
                .unwrap(),
                stores.fingerprint
            );
            unchanged(&path, &hash);
        }
        for (alias, password, expected) in [
            (
                "中文release",
                "incorrect",
                FailureCode::KeystorePasswordInvalid,
            ),
            ("missing", password, FailureCode::KeyAliasNotFound),
        ] {
            let error = extract_keystore_cert_fingerprint_with_context(
                &context,
                &stores.java,
                &path,
                alias,
                password,
                Some(kind),
            )
            .unwrap_err();
            assert_eq!(Diagnostic::from_error(&error).code, expected);
            assert!(!error.to_string().contains(password));
            unchanged(&path, &hash);
        }
        if kind == "PKCS12" {
            let error = extract_keystore_cert_fingerprint_with_context(
                &context,
                &stores.java,
                &path,
                "secret",
                password,
                Some(kind),
            )
            .unwrap_err();
            assert!(error.to_string().contains("没有证书"));
            unchanged(&path, &hash);
        }
    }
    println!("只读证书回归：双类型、中文及特殊字符、全部条目、独立指纹、错误分类及原件不变均通过");
}

#[test]
#[ignore = "需正式 JAR 和显式 Java 8／17；验证预算及固定读取失败，不接入真实资料"]
fn 空库损坏不支持类型及结果超限均拒绝且原件不变() {
    let stores = Stores::new();
    let context = stores.context();
    for (variant, code) in [
        ("empty", FailureCode::KeyAliasNotFound),
        ("many", FailureCode::ToolProcessFailed),
        ("long", FailureCode::ToolProcessFailed),
        ("wide", FailureCode::ToolProcessFailed),
    ] {
        let path = stores.create("JKS", variant, INITIAL);
        let hash = Sha256::digest(fs::read(&path).unwrap()).into();
        let error = query_keystore_aliases_with_context(
            &context,
            &stores.java,
            &path,
            INITIAL,
            Some("JKS"),
        )
        .unwrap_err();
        assert_eq!(Diagnostic::from_error(&error).code, code);
        if variant != "empty" {
            assert!(error.to_string().contains("超出预算"));
        }
        unchanged(&path, &hash);
    }
    let invalid = stores.root.path().join("corrupt.jks");
    fs::write(&invalid, b"invalid").unwrap();
    let hash = Sha256::digest(fs::read(&invalid).unwrap()).into();
    for kind in ["JKS", "unsupported"] {
        let error = query_keystore_aliases_with_context(
            &context,
            &stores.java,
            &invalid,
            INITIAL,
            Some(kind),
        )
        .unwrap_err();
        assert_eq!(
            Diagnostic::from_error(&error).code,
            FailureCode::KeystoreFormatInvalid
        );
        unchanged(&invalid, &hash);
    }
    let error = query_keystore_aliases_with_context(
        &context,
        &stores.java,
        &stores.root.path().join("missing"),
        INITIAL,
        Some("JKS"),
    )
    .unwrap_err();
    assert_eq!(
        Diagnostic::from_error(&error).code,
        FailureCode::FileNotFound
    );
    let empty = stores.create("PKCS12", "empty", INITIAL);
    let hash = Sha256::digest(fs::read(&empty).unwrap()).into();
    let error = query_keystore_aliases_with_context(
        &context,
        &stores.java,
        &empty,
        INITIAL,
        Some("PKCS12"),
    )
    .unwrap_err();
    assert_eq!(
        Diagnostic::from_error(&error).code,
        FailureCode::KeyAliasNotFound
    );
    unchanged(&empty, &hash);
    let error = query_keystore_aliases_with_context(
        &context,
        &stores.java,
        stores.root.path(),
        INITIAL,
        Some("JKS"),
    )
    .unwrap_err();
    assert_eq!(
        Diagnostic::from_error(&error).code,
        FailureCode::KeystoreFormatInvalid
    );
    println!("只读拒绝回归：空库、类型、损坏、缺失及预算均拒绝，未返回部分结果或改变原件");
}

#[test]
#[ignore = "需正式 JAR 与真实 JDK；仅对本次合成文件设置读取拒绝，不改变用户权限"]
fn 证书读取拒绝不改变原件且错误口令不能进入请求文件() {
    let stores = Stores::new();
    let path = stores.create("JKS", "normal", UNICODE);
    let context = stores.context();
    let hash = Sha256::digest(fs::read(&path).unwrap()).into();
    let before = fs::read_dir(stores.root.path()).unwrap().count();
    let error = query_keystore_aliases_with_context(
        &context,
        &stores.java,
        &path,
        "secret\0must-not-leak",
        Some("JKS"),
    )
    .unwrap_err();
    assert!(!error.to_string().contains("secret"));
    assert_eq!(
        fs::read_dir(stores.root.path()).unwrap().count(),
        before,
        "无效环境数据不能准备请求或启动查询"
    );
    #[cfg(unix)]
    let restore = {
        use std::os::unix::fs::PermissionsExt;
        assert_ne!(
            unsafe { libc::geteuid() },
            0,
            "特权进程不能用模式位证明读取拒绝"
        );
        let restore = fs::metadata(&path).unwrap().permissions();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o000)).unwrap();
        restore
    };
    #[cfg(windows)]
    let restore = {
        let control = permissions::control(&path, false);
        permissions::set_acl(&control, "D:P");
        control
    };
    let result =
        query_keystore_aliases_with_context(&context, &stores.java, &path, UNICODE, Some("JKS"));
    #[cfg(unix)]
    fs::set_permissions(&path, restore).unwrap();
    #[cfg(windows)]
    permissions::make_public(&restore);
    let error = result.unwrap_err();
    assert_eq!(
        Diagnostic::from_error(&error).code,
        FailureCode::PermissionDenied
    );
    unchanged(&path, &hash);
    // 所有请求文件均属于本测试根；即使口令为中文也不得落入传输文件。
    for entry in fs::read_dir(stores.root.path()).unwrap() {
        let request = entry.unwrap().path().join("request.bin");
        if request.is_file() {
            let bytes = fs::read(request).unwrap();
            assert!(!bytes
                .windows(UNICODE.len())
                .any(|part| part == UNICODE.as_bytes()));
            assert!(!bytes
                .windows(INITIAL.len())
                .any(|part| part == INITIAL.as_bytes()));
        }
    }
    println!("读取拒绝回归：固定权限分类、原件不变、口令不落请求均通过");
}

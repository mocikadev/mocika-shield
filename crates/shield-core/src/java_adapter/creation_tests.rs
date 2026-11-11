//! 正式新建接口仅操作合成资料；独立原工具指纹与标准接口核对实际内容。
use super::certificate_fixture::Stores;
use super::*;
use crate::keytool::{create_keystore_draft_with_context, CreateKeystoreOptions};

fn options<'a>(kind: &'a str, store: &'a str, key: &'a str) -> CreateKeystoreOptions<'a> {
    CreateKeystoreOptions {
        kind,
        alias: "中文Release😀",
        keystore_password: store,
        key_password: key,
        dname: "CN=中文\\, 应用😀+UID=123,OU=研发,O=测试\\+组织,L=北京,ST=北京,C=CN",
        validity_days: 2,
        key_size: 2048,
    }
}

#[test]
#[ignore = "需正式 JAR 和显式 Java 8／17；只生成临时合成证书"]
fn 新建双格式中文主题密码与独立原工具指纹完整核验() {
    let stores = Stores::new();
    let context = stores.context();
    let keytool = crate::utils::find_keytool().unwrap();
    for (index, (kind, store, key)) in [
        ("JKS", "中文仓库密码123", "独立密钥密码123"),
        ("JKS", "ascii-store-password", ""),
        ("PKCS12", "ascii-store-password", ""),
        ("PKCS12", "ascii-store-password", "ascii-store-password"),
    ]
    .into_iter()
    .enumerate()
    {
        let mut options = options(kind, store, key);
        if index == 1 {
            options.key_size = 4096;
        }
        let output = stores
            .root
            .path()
            .join(format!("新证书😀-{index}.keystore"));
        let before: std::collections::HashSet<_> = fs::read_dir(stores.root.path())
            .unwrap()
            .map(|item| item.unwrap().path())
            .collect();
        let created =
            create_keystore_draft_with_context(&context, &stores.java, &keytool, &output, &options)
                .unwrap();
        assert_eq!(created.alias, "中文release😀");
        stores.verify_created(&output, &created, &options);
        let raw = fs::read_dir(stores.root.path())
            .unwrap()
            .map(|item| item.unwrap().path())
            .find(|path| !before.contains(path) && path.join("generated.keystore").is_file())
            .unwrap()
            .join("generated.keystore");
        assert_eq!(
            crate::extract_keystore_cert_fingerprint(&raw, "generated", store, Some(kind)).unwrap(),
            created.fingerprint
        );
        let hash = sha2::Sha256::digest(fs::read(&output).unwrap());
        assert!(create_keystore_draft_with_context(
            &context,
            &stores.java,
            &keytool,
            &output,
            &options
        )
        .is_err());
        assert_eq!(sha2::Sha256::digest(fs::read(&output).unwrap()), hash);
    }
    println!("新建证书回归：双格式、中文及多属性转义主题、实际 Alias、密码分离／留空、密钥位数、独立内容与指纹、原件不覆盖均通过");
}

#[test]
#[ignore = "需正式 JAR 与显式 JDK；仅测试新建失败和无业务输出"]
fn 不支持的密码主题类型及参数不产生目标或泄露口令() {
    let stores = Stores::new();
    let context = stores.context();
    let keytool = crate::utils::find_keytool().unwrap();
    let output = stores.root.path().join("must-not-exist.jks");
    for mut input in [
        options("PKCS12", "中文密码123", ""),
        options("PKCS12", "ascii-store-password", "different-password"),
        options("unsupported", "ascii-store-password", ""),
        options("JKS", "short", ""),
        options("JKS", "hidden\0password", ""),
        options("JKS", "ascii-store-password", ""),
    ] {
        if input.kind == "JKS" && input.keystore_password == "ascii-store-password" {
            input.dname = "invalid subject";
        }
        let result =
            create_keystore_draft_with_context(&context, &stores.java, &keytool, &output, &input);
        assert!(result.is_err());
        let error = result.err().unwrap().to_string();
        assert!(!error.contains(input.keystore_password));
        assert!(!output.exists());
    }
    // 新请求中的密码只存在局部环境；本次借用根保留材料直到所有者回收。
    for entry in fs::read_dir(stores.root.path()).unwrap() {
        let request = entry.unwrap().path().join("request.bin");
        if request.is_file() {
            let bytes = fs::read(request).unwrap();
            for password in ["中文密码123", "ascii-store-password", "different-password"] {
                assert!(!bytes
                    .windows(password.len())
                    .any(|part| part == password.as_bytes()));
            }
        }
    }
    println!("新建拒绝回归：密码组合、损坏主题、类型及非法输入拒绝，目标未生成，口令未落请求");
}

#[test]
#[ignore = "需正式 JAR；只测试本次原始生成材料的整理拒绝，不使用历史证书"]
fn 整理失败不修改原始生成材料或已有目标() {
    let stores = Stores::new();
    let context = stores.context();
    let options = options("JKS", "isolated-store-password", "isolated-key-password");
    let output = stores.root.path().join("created.jks");
    create_keystore_draft_with_context(
        &context,
        &stores.java,
        &crate::utils::find_keytool().unwrap(),
        &output,
        &options,
    )
    .unwrap();
    let raw = fs::read_dir(stores.root.path())
        .unwrap()
        .map(|entry| entry.unwrap().path().join("generated.keystore"))
        .find(|path| path.is_file())
        .unwrap();
    let original = fs::read(&raw).unwrap();
    let target = stores.root.path().join("finish-target.jks");
    let days = options.validity_days.to_string();
    let size = options.key_size.to_string();
    let values = [
        "keystore_create_finish",
        options.kind,
        options.alias,
        options.dname,
        &days,
        &size,
        raw.to_str().unwrap(),
        target.to_str().unwrap(),
    ];
    for bad_password in [true, false] {
        if !bad_password {
            fs::write(&target, b"existing-target").unwrap();
        }
        let mut command = context.prepare_request(&stores.java, &values).unwrap();
        command
            .env(
                crate::java_process::KS_PASS_ENV,
                if bad_password {
                    "incorrect-store-password"
                } else {
                    options.keystore_password
                },
            )
            .env(crate::java_process::KEY_PASS_ENV, options.key_password);
        let (status, bytes) = JavaToolContext::collect(command, 65536).unwrap();
        assert!(
            super::super::certificates::decode::<crate::keytool::CreatedKeystore>(
                &bytes,
                status.code(),
                values[0]
            )
            .is_err()
        );
        assert_eq!(fs::read(&raw).unwrap(), original);
        if bad_password {
            assert!(!target.exists());
        } else {
            assert_eq!(fs::read(&target).unwrap(), b"existing-target");
        }
    }
    println!("整理拒绝回归：错误口令不生成结果，已有目标与原始生成材料均不修改");
}

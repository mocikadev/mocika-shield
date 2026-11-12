//! 只使用自有 APK 和临时证书；不接公开入口、资料库或默认资源选择。
use super::{
    certificate_fixture::{Stores, INITIAL},
    signing_fixture::*,
    *,
};
use crate::{
    keytool::{create_keystore_draft_with_context, CreateKeystoreOptions},
    signing::{
        sign_apk, sign_apk_with_context, KeystoreType, SignOptions, SigningProgressStep,
        SigningVersions,
    },
};

fn options(
    input: &Path,
    store: &Path,
    jar: &Path,
    kind: &str,
    alias: &str,
    store_pass: &str,
    key_pass: &str,
) -> SignOptions {
    SignOptions {
        apk_path: input.to_owned(),
        output_path: Some(input.with_file_name("中文 指定输出.apk")),
        keystore_path: store.to_owned(),
        key_alias: alias.into(),
        keystore_password: store_pass.into(),
        key_password: key_pass.into(),
        apksigner_path: Some(jar.to_owned()),
        keystore_type: KeystoreType::parse(kind),
        signing_versions: SigningVersions::default(),
    }
}

#[test]
#[ignore = "需要正式 JAR、真实 SHIELD_TEST_APK／SHIELD_TEST_ZIPALIGN 和显式 Java 8／17"]
fn 上下文签名中文真实apk与原地覆盖经原工具独立验签() {
    let stores = Stores::new();
    let root = stores.root.path();
    let workspace = root.join("中文 私有工作根");
    create_workspace(&workspace).unwrap();
    let context = JavaToolContext::new(&root.join("adapter.jar"), &workspace).unwrap();
    let input = input(root, "中文 输入😀.apk");
    let original = fs::read(&input).unwrap();
    let content = contents(&input);
    let signer = root.join("中文 签名工具.jar");
    fs::copy(crate::utils::find_apksigner().unwrap(), &signer).unwrap();
    let keytool = crate::utils::find_keytool().unwrap();
    for (index, (kind, store_pass, key_pass)) in [
        ("JKS", "中文仓库密码123", "独立密钥密码123"),
        ("JKS", "ascii-store-password", ""),
        ("PKCS12", "ascii-store-password", ""),
    ]
    .into_iter()
    .enumerate()
    {
        let store = root.join(format!("中文 证书😀-{index}.keystore"));
        let create = CreateKeystoreOptions {
            kind,
            alias: "中文Release😀",
            keystore_password: store_pass,
            key_password: key_pass,
            dname: "CN=签名验证😀,O=测试",
            validity_days: 2,
            key_size: 2048,
        };
        let created =
            create_keystore_draft_with_context(&context, &stores.java, &keytool, &store, &create)
                .unwrap();
        let before_store = fs::read(&store).unwrap();
        // 服务层的留空 Key 密码已按原约定解析为 Store 密码，核心不另改密码规则。
        let key_pass = if key_pass.is_empty() {
            store_pass
        } else {
            key_pass
        };
        let mut opts = options(
            &input,
            &store,
            &signer,
            kind,
            &created.alias,
            store_pass,
            key_pass,
        );
        let mut progress = Vec::new();
        sign_apk_with_context(&context, &stores.java, &opts, |step| {
            progress.push(match step {
                SigningProgressStep::Prepare => 0,
                SigningProgressStep::Align => 1,
                SigningProgressStep::Sign => 2,
                SigningProgressStep::Commit => 3,
            });
            Ok(())
        })
        .unwrap();
        assert_eq!(progress, [0, 1, 2, 3]);
        let output = opts.output_path.as_ref().unwrap();
        assert_eq!(contents(output), content);
        assert_eq!(
            crate::extract_apk_cert_fingerprint_with_context(
                &context,
                &stores.java,
                output,
                &signer
            )
            .unwrap(),
            created.fingerprint
        );
        independently_verify(root, &stores.java, output, &created.fingerprint);
        assert_eq!(fs::read(&input).unwrap(), original);
        // 普通签名允许覆盖指定输出；适配不能偷偷改成无覆盖提交。
        fs::write(output, b"previous-output").unwrap();
        sign_apk_with_context(&context, &stores.java, &opts, |_| Ok(())).unwrap();
        independently_verify(root, &stores.java, output, &created.fingerprint);
        assert_eq!(contents(output), content);
        if index == 0 {
            opts.signing_versions.v4 = true;
            sign_apk_with_context(&context, &stores.java, &opts, |_| Ok(())).unwrap();
            assert!(
                output.with_file_name("中文 指定输出.apk.idsig").is_file(),
                "显式普通签名也必须保留 V4 副产物"
            );
            independently_verify(root, &stores.java, output, &created.fingerprint);
            opts.signing_versions.v4 = false;
        }
        let inplace = root.join(format!("中文 原地😀-{index}.apk"));
        fs::copy(&input, &inplace).unwrap();
        opts.apk_path = inplace.clone();
        opts.output_path = None;
        sign_apk_with_context(&context, &stores.java, &opts, |_| Ok(())).unwrap();
        independently_verify(root, &stores.java, &inplace, &created.fingerprint);
        assert_eq!(contents(&inplace), content);
        if index == 0 {
            let tampered = root.join("中文 损坏签名.apk");
            fs::copy(&inplace, &tampered).unwrap();
            let file = fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open(&tampered)
                .unwrap();
            let mut zip = zip::ZipWriter::new_append(file).unwrap();
            zip.start_file(
                "assets/adapter-signing-check/tampered.txt",
                zip::write::SimpleFileOptions::default(),
            )
            .unwrap();
            zip.write_all(b"unsigned-change").unwrap();
            zip.finish().unwrap();
            let before = fs::read(&tampered).unwrap();
            let error = crate::extract_apk_cert_fingerprint_with_context(
                &context,
                &stores.java,
                &tampered,
                &signer,
            )
            .unwrap_err();
            assert_eq!(
                Diagnostic::from_error(&error).tool,
                Some(crate::diagnostic::ToolName::Apksigner)
            );
            assert!(!format!("{error:#}").contains(tampered.to_str().unwrap()));
            assert_eq!(fs::read(&tampered).unwrap(), before, "验签失败只能只读");
        }
        assert_eq!(fs::read(&store).unwrap(), before_store, "签名不能改写证书");
        assert_eq!(fs::read(&input).unwrap(), original);
    }
    requests_without_secrets(
        root,
        &["中文仓库密码123", "独立密钥密码123", "ascii-store-password"],
    );
    drop(context);
    assert!(
        workspace.exists() && fs::read_dir(&workspace).unwrap().count() > 0,
        "上下文不得清理借用材料"
    );
    println!("真实签名回归：中文 JAR／工作根及含补充字符的业务路径／Alias，双类型与分离密码，指定输出／覆盖／原地签名、内容与压缩策略、独立 V1～V3 和 SDK 16 KB 对齐、原件不变均通过；非设备安装验收");
}

#[test]
#[ignore = "需要正式 JAR、自有真实 APK 和显式 Java；仅验证临时材料失败及阶段取消"]
fn 上下文签名错误与阶段取消不改原包且保留借用材料() {
    let stores = Stores::new();
    let root = stores.root.path();
    let context = stores.context();
    let input = input(root, "中文 拒绝输入.apk");
    let original = fs::read(&input).unwrap();
    let store = stores.create("JKS", "normal", "隔离中文密码123");
    let before_store = fs::read(&store).unwrap();
    let mut opts = options(
        &input,
        &store,
        &crate::utils::find_apksigner().unwrap(),
        "JKS",
        "含,引号\"😀",
        "隔离中文密码123",
        "隔离中文密码123",
    );
    opts.output_path = None;
    for (store_pass, key_pass, alias, code) in [
        (
            "wrong-store-password",
            "隔离中文密码123",
            "含,引号\"😀",
            FailureCode::KeystorePasswordInvalid,
        ),
        (
            "隔离中文密码123",
            "wrong-key-password",
            "含,引号\"😀",
            FailureCode::KeystorePasswordInvalid,
        ),
        (
            "隔离中文密码123",
            "隔离中文密码123",
            "missing-alias",
            FailureCode::KeyAliasNotFound,
        ),
    ] {
        opts.keystore_password = store_pass.into();
        opts.key_password = key_pass.into();
        opts.key_alias = alias.into();
        let error = sign_apk_with_context(&context, &stores.java, &opts, |_| Ok(())).unwrap_err();
        let diagnostic = Diagnostic::from_error(&error);
        assert_eq!(diagnostic.code, code);
        assert_eq!(
            diagnostic.tool,
            Some(crate::diagnostic::ToolName::Apksigner)
        );
        assert!(diagnostic.exit_code.is_some_and(|code| code != 0));
        for hidden in [store_pass, key_pass, alias, store.to_str().unwrap()] {
            assert!(!format!("{error:#}").contains(hidden));
        }
        assert_eq!(fs::read(&input).unwrap(), original);
        assert_eq!(fs::read(&store).unwrap(), before_store);
    }
    for reject in [0, 1, 2] {
        let mut reached = Vec::new();
        assert!(
            sign_apk_with_context(&context, &stores.java, &opts, |step| {
                let index = match step {
                    SigningProgressStep::Prepare => 0,
                    SigningProgressStep::Align => 1,
                    SigningProgressStep::Sign => 2,
                    SigningProgressStep::Commit => 3,
                };
                reached.push(index);
                if index == reject {
                    Err("阶段已取消".into())
                } else {
                    Ok(())
                }
            })
            .is_err()
        );
        assert_eq!(reached, (0..=reject).collect::<Vec<_>>());
        assert_eq!(fs::read(&input).unwrap(), original);
    }
    let before = fs::read_dir(root).unwrap().count();
    opts.apksigner_path = None;
    assert!(sign_apk_with_context(&context, &stores.java, &opts, |_| Ok(())).is_err());
    assert_eq!(
        fs::read_dir(root).unwrap().count(),
        before,
        "显式入口缺工具不能转向全局搜索"
    );
    requests_without_secrets(
        root,
        &[
            "隔离中文密码123",
            "wrong-store-password",
            "wrong-key-password",
        ],
    );
    drop(context);
    assert!(fs::read_dir(root).unwrap().any(|entry| entry
        .unwrap()
        .path()
        .join("aligned.apk")
        .exists()));
    println!("签名拒绝回归：错误 Store／Key 密码及 Alias、三处进度取消、缺工具不搜索均通过；原包和证书不变、密码不落请求、借用材料保留；未代替运行中整组停止验收");
}

#[test]
#[ignore = "需要真实 APK、JDK 和原签名工具；以 ASCII 材料复验旧默认入口及 V4 副产物"]
fn 旧签名入口保持指定输出原地覆盖和v4参数行为() {
    let stores = Stores::new();
    let root = stores.root.path();
    let input = input(root, "legacy-input.apk");
    let original = fs::read(&input).unwrap();
    let content = contents(&input);
    let mut opts = options(
        &input,
        &root.join("bootstrap.jks"),
        &crate::utils::find_apksigner().unwrap(),
        "JKS",
        "release",
        INITIAL,
        INITIAL,
    );
    let output = root.join("legacy-output.apk");
    opts.output_path = Some(output.clone());
    sign_apk(&opts).unwrap();
    independently_verify(root, &stores.java, &output, &stores.fingerprint);
    fs::write(&output, b"previous-output").unwrap();
    opts.signing_versions.v4 = true;
    sign_apk(&opts).unwrap();
    assert!(
        root.join("legacy-output.apk.idsig").is_file(),
        "普通入口必须保留 V4 副产物"
    );
    independently_verify(root, &stores.java, &output, &stores.fingerprint);
    assert_eq!(contents(&output), content);
    assert_eq!(fs::read(&input).unwrap(), original);
    let signed = fs::read(&output).unwrap();
    let idsig = root.join("legacy-output.apk.idsig");
    let attachment = fs::read(&idsig).unwrap();
    assert!(crate::sign_apk_with_progress(&opts, |step| {
        if step == SigningProgressStep::Commit {
            Err("提交前取消".into())
        } else {
            Ok(())
        }
    })
    .is_err());
    assert_eq!(fs::read(&output).unwrap(), signed);
    assert_eq!(fs::read(&idsig).unwrap(), attachment);
    opts.signing_versions.v4 = false;
    opts.output_path = None;
    sign_apk(&opts).unwrap();
    independently_verify(root, &stores.java, &input, &stores.fingerprint);
    assert_eq!(contents(&input), content);
    println!("旧入口回归：ASCII 真实 APK 指定输出／覆盖／原地签名、内容与独立验签／对齐、V4 副产物保留均通过；不扩大受控任务 V1～V3 范围");
}

#[cfg(windows)]
#[test]
#[ignore = "需 Windows 原生专项的异卷工作区与临时根，验证拒绝前不删除本次自有 APK"]
fn 异卷原地签名在对齐和工具启动前拒绝且原包不变() {
    let stores = Stores::new();
    let source = std::path::PathBuf::from(std::env::var_os("SHIELD_TEST_APK").unwrap());
    let target_root = tempfile::Builder::new()
        .prefix("adapter-volume-")
        .tempdir_in(source.parent().unwrap())
        .unwrap();
    let input = input(target_root.path(), "inplace-must-survive.apk");
    let original = fs::read(&input).unwrap();
    let context = stores.context();
    let directory = crate::java_directory::open_directory(stores.root.path()).unwrap();
    let file = fs::File::open(&input).unwrap();
    assert_ne!(
        crate::java_directory::identity(&directory).unwrap().0,
        crate::java_directory::identity(&file).unwrap().0,
        "专项必须实际具备异卷条件，不能记作已验证"
    );
    drop(file);
    let mut options = options(
        &input,
        &stores.root.path().join("bootstrap.jks"),
        &crate::utils::find_apksigner().unwrap(),
        "JKS",
        "release",
        INITIAL,
        INITIAL,
    );
    options.output_path = None;
    let mut reached = Vec::new();
    let error = sign_apk_with_context(&context, &stores.java, &options, |step| {
        reached.push(step);
        Ok(())
    })
    .unwrap_err();
    assert!(matches!(reached.as_slice(), [SigningProgressStep::Prepare]));
    assert_eq!(
        Diagnostic::from_error(&error).code,
        FailureCode::ToolProcessFailed
    );
    assert_eq!(fs::read(&input).unwrap(), original);
    assert!(!fs::read_dir(stores.root.path()).unwrap().any(|entry| entry
        .unwrap()
        .path()
        .join("aligned.apk")
        .exists()));
    // 普通入口拥有工作根，应直接选择输入所在卷；显式入口不得自动改变借用根。
    let content = contents(&input);
    let mut ordinary_steps = Vec::new();
    crate::sign_apk_with_progress(&options, |step| {
        ordinary_steps.push(step);
        Ok(())
    })
    .unwrap();
    assert!(matches!(
        ordinary_steps.as_slice(),
        [
            SigningProgressStep::Prepare,
            SigningProgressStep::Align,
            SigningProgressStep::Sign,
            SigningProgressStep::Commit,
        ]
    ));
    independently_verify(
        target_root.path(),
        &stores.java,
        &input,
        &stores.fingerprint,
    );
    assert_eq!(contents(&input), content);
    println!("异卷结果：实际卷不同，显式借用根在签名前拒绝且原包不变；普通入口选择输入同卷暂存并完成签名，独立验签及内容核验通过");
}

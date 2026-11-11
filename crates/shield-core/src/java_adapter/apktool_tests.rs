//! 自有真实 APK 的解包／修改／重建；不代表壳注入或 Android 设备验收。
use super::{signing_fixture::*, *};
use std::{collections::BTreeMap, path::PathBuf};

fn adapter() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tools/java-adapter/build/java-adapter.jar")
}

fn payload(path: &Path) -> BTreeMap<String, [u8; 32]> {
    contents(path)
        .into_iter()
        .filter_map(|(name, (digest, _))| {
            (name.ends_with(".dex") || name.starts_with("assets/") || name.starts_with("lib/"))
                .then_some((name, digest))
        })
        .collect()
}

fn minimum_sdk(metadata: &str) -> u32 {
    metadata
        .lines()
        .find_map(|line| line.trim().strip_prefix("minSdkVersion:"))
        .unwrap()
        .trim()
        .trim_matches(['\'', '"'])
        .parse()
        .unwrap()
}

fn independent(root: &Path, java: &Path, candidate: &Path, name: &str) -> String {
    let input = root.join(format!("{name}.apk"));
    fs::copy(candidate, &input).unwrap();
    let output = root.join(name);
    let framework = root.join(format!("{name}-framework"));
    create_workspace(&framework).unwrap();
    let jar = root.join("independent-apktool.jar");
    if !jar.exists() {
        fs::copy(crate::utils::find_apktool().unwrap(), &jar).unwrap();
    }
    // 原 argv 对照全部使用 ASCII，避免把独立检查本身的旧 Windows 编码问题混入结果。
    let result = java_command(java)
        .current_dir(root)
        .args(["-Djava.io.tmpdir=.", "-jar"])
        .arg(jar)
        .arg("d")
        .arg(input)
        .arg("--no-src")
        .arg("-o")
        .arg(&output)
        .arg("-p")
        .arg(framework)
        .output()
        .unwrap();
    assert!(result.status.success(), "独立原工具未能重新解包候选");
    fs::read_to_string(output.join("AndroidManifest.xml")).unwrap()
}

#[test]
#[ignore = "需正式 JAR、自有 APK、SDK 及 Java 8／17；真实修改重建后独立重解包验签"]
fn 上下文解包重建保留内容并隔离框架缓存和候选() {
    let stores = certificate_fixture::Stores::new();
    let root = strip_unc_prefix(stores.root.path());
    let work = root.join("中文 私有重建工作根");
    create_workspace(&work).unwrap();
    let work = strip_unc_prefix(&fs::canonicalize(work).unwrap());
    let context = JavaToolContext::new(&adapter(), &work).unwrap();
    let input = input(&root, "中文 输入😀.apk");
    let original = fs::read(&input).unwrap();
    let expected = payload(&input);
    let tool = root.join("中文 解包工具.jar");
    fs::copy(crate::utils::find_apktool().unwrap(), &tool).unwrap();
    let project = context.decode_apk(&stores.java, &tool, &input).unwrap();
    assert!(project.directory().starts_with(&work));
    let manifest = project.directory().join("AndroidManifest.xml");
    let text = fs::read_to_string(&manifest).unwrap();
    let start = text.find("<application").unwrap();
    let end = start + text[start..].find('>').unwrap();
    let mut changed = text.clone();
    if let Some(offset) = text[start..end].find("android:label=\"") {
        let value = start + offset + "android:label=\"".len();
        let last = value + text[value..].find('"').unwrap();
        changed.replace_range(value..last, "无损重建验证");
    } else {
        changed.insert_str(
            start + "<application".len(),
            " android:label=\"无损重建验证\"",
        );
    }
    // 高最低版本强制覆盖“默认验签跳过 V1”的真实反例；只修改本次测试副本。
    fs::write(&manifest, changed).unwrap();
    // apktool 将 uses-sdk 移到项目元数据，重建时再写回二进制清单。
    let metadata_path = project.directory().join("apktool.yml");
    let metadata = fs::read_to_string(&metadata_path).unwrap();
    let sdk = minimum_sdk(&metadata).max(28);
    let line = metadata
        .lines()
        .find(|line| line.trim_start().starts_with("minSdkVersion:"))
        .unwrap();
    let prefix = &line[..line.len() - line.trim_start().len()];
    fs::write(
        &metadata_path,
        metadata.replacen(line, &format!("{prefix}minSdkVersion: {sdk}"), 1),
    )
    .unwrap();
    let first = project.rebuild().unwrap();
    let bytes = fs::read(&first).unwrap();
    assert!(first.starts_with(&work));
    assert_eq!(payload(&first), expected);
    let text = independent(&root, &stores.java, &first, "verify-one");
    assert!(text.contains("android:label=\"无损重建验证\""));
    assert_eq!(
        minimum_sdk(&fs::read_to_string(root.join("verify-one/apktool.yml")).unwrap()),
        sdk
    );
    let second = project.rebuild().unwrap();
    assert_ne!(first, second, "重复重建必须生成新候选，不覆盖已有候选");
    assert_eq!(fs::read(&first).unwrap(), bytes);
    assert_eq!(payload(&second), expected);
    let store = stores.create("JKS", "normal", certificate_fixture::UNICODE);
    let fingerprint = crate::keytool::extract_keystore_cert_fingerprint_with_context(
        &context,
        &stores.java,
        &store,
        "含,引号\"😀",
        certificate_fixture::UNICODE,
        Some("JKS"),
    )
    .unwrap();
    let signed = root.join("signed.apk");
    crate::sign_apk_with_context(
        &context,
        &stores.java,
        &crate::SignOptions {
            apk_path: first.clone(),
            output_path: Some(signed.clone()),
            keystore_path: store.clone(),
            key_alias: "含,引号\"😀".into(),
            keystore_password: certificate_fixture::UNICODE.into(),
            key_password: certificate_fixture::UNICODE.into(),
            apksigner_path: Some(crate::utils::find_apksigner().unwrap()),
            keystore_type: crate::KeystoreType::Jks,
            signing_versions: crate::SigningVersions::default(),
        },
        |_| Ok(()),
    )
    .unwrap();
    independently_verify(&root, &stores.java, &signed, &fingerprint);
    assert_eq!(fs::read(&input).unwrap(), original);
    assert_eq!(fs::read(&first).unwrap(), bytes, "签名不能改写重建候选");
    assert!(
        walkdir::WalkDir::new(&work)
            .into_iter()
            .any(|entry| entry.unwrap().file_name() == "1.apk"),
        "框架必须缓存于本次工作根"
    );
    drop(project);
    drop(context);
    assert!(
        manifest.is_file() && second.is_file(),
        "上下文不自动清理借用材料"
    );
    println!("真实解包重建结果：中文工具／工作根、补充字符输入、Manifest 修改、DEX／资源／Native 字节保留、独立原工具重解包、V1～V3 验签及 SDK 对齐、私有框架缓存和候选保留通过；不含壳注入或设备安装");
}

#[test]
#[ignore = "需正式 JAR、自有 APK 及显式 Java；无效输入和真实资源编译失败不发布"]
fn 上下文解包及重建真实失败保留输入与已有候选() {
    let root = private_root();
    let root = strip_unc_prefix(root.path());
    let java = crate::utils::find_java().unwrap();
    let jar = crate::utils::find_apktool().unwrap();
    let context = JavaToolContext::new(&adapter(), &root).unwrap();
    let corrupt = root.join("中文 损坏.apk");
    fs::write(&corrupt, b"invalid-apk").unwrap();
    let error = match context.decode_apk(&java, &jar, &corrupt) {
        Ok(_) => panic!("坏包不能成功"),
        Err(error) => error,
    };
    assert_eq!(
        Diagnostic::from_error(&error).tool,
        Some(crate::diagnostic::ToolName::Apktool)
    );
    assert!(!format!("{error:#}").contains("中文 损坏"));
    assert_eq!(fs::read(&corrupt).unwrap(), b"invalid-apk");
    let input = input(&root, "中文 原包.apk");
    let original = fs::read(&input).unwrap();
    let project = context.decode_apk(&java, &jar, &input).unwrap();
    let first = project.rebuild().unwrap();
    let bytes = fs::read(&first).unwrap();
    fs::write(
        project.directory().join("AndroidManifest.xml"),
        b"<broken-resource",
    )
    .unwrap();
    let error = project.rebuild().unwrap_err();
    let diagnostic = Diagnostic::from_error(&error);
    assert_eq!(diagnostic.tool, Some(crate::diagnostic::ToolName::Apktool));
    assert!(matches!(
        diagnostic.code,
        FailureCode::ApkRepackFailed | FailureCode::ManifestRebuildFailed
    ));
    assert_eq!(fs::read(&input).unwrap(), original);
    assert_eq!(fs::read(&first).unwrap(), bytes);
    assert!(project.directory().is_dir());
    println!(
        "真实解包重建失败结果：坏包及无效 Manifest 均拒绝，原包和已有候选不变，材料留给外层回收"
    );
}

//! 普通签名和只读检查的真实默认上下文，不使用受控任务的显式上下文替代。
use super::{
    certificate_fixture::{Stores, INITIAL},
    signing_fixture::*,
    *,
};
use crate::{
    signing::{sign_apk_default, SigningProgressStep},
    KeystoreType, SignOptions, SigningVersions,
};
use std::path::PathBuf;

fn sign(
    opts: &SignOptions,
    progress: impl FnMut(SigningProgressStep) -> Result<(), String>,
) -> Result<()> {
    #[cfg(windows)]
    return crate::sign_apk_with_progress(opts, progress);
    #[cfg(not(windows))]
    sign_apk_default(opts, progress)
}

#[test]
#[ignore = "需正式资源、自有 APK、真实 Java 8／17 及 SDK zipalign"]
fn 普通签名中文材料覆盖原地签名验签和失败保留() {
    let stores = Stores::new();
    let root = stores.root.path();
    let input = input(root, "普通中文 输入😀.apk");
    let original = fs::read(&input).unwrap();
    let content = contents(&input);
    let store = stores.create("JKS", "normal", "普通中文口令123");
    let before_store = fs::read(&store).unwrap();
    let output = root.join("普通中文 输出😀.apk");
    let mut opts = SignOptions {
        apk_path: input.clone(),
        output_path: Some(output.clone()),
        keystore_path: store.clone(),
        key_alias: "含,引号\"😀".into(),
        keystore_password: "普通中文口令123".into(),
        key_password: "普通中文口令123".into(),
        apksigner_path: Some(crate::utils::find_apksigner().unwrap()),
        keystore_type: KeystoreType::Jks,
        signing_versions: SigningVersions::default(),
    };
    let mut stages = vec![];
    sign(&opts, |stage| {
        stages.push(stage);
        Ok(())
    })
    .unwrap();
    assert_eq!(
        stages,
        [
            SigningProgressStep::Prepare,
            SigningProgressStep::Align,
            SigningProgressStep::Sign,
            SigningProgressStep::Commit,
        ]
    );
    assert_eq!(contents(&output), content);
    independently_verify(root, &stores.java, &output, &stores.fingerprint);
    assert!(
        crate::apk_inspect::check_apk_default(&output, opts.apksigner_path.as_deref())
            .unwrap()
            .is_signed
    );
    #[cfg(windows)]
    assert_eq!(
        crate::extract_apk_cert_fingerprint(&output, opts.apksigner_path.as_deref()).unwrap(),
        stores.fingerprint
    );
    fs::write(&output, b"old-output").unwrap();
    opts.signing_versions.v4 = true;
    sign(&opts, |_| Ok(())).unwrap();
    assert!(root.join("普通中文 输出😀.apk.idsig").is_file());
    independently_verify(root, &stores.java, &output, &stores.fingerprint);
    let signed_output = fs::read(&output).unwrap();
    let attachment = root.join("普通中文 输出😀.apk.idsig");
    let old_attachment = fs::read(&attachment).unwrap();
    assert!(sign(&opts, |stage| {
        if stage == SigningProgressStep::Commit {
            Err("提交前取消".into())
        } else {
            Ok(())
        }
    })
    .is_err());
    assert_eq!(fs::read(&output).unwrap(), signed_output);
    assert_eq!(fs::read(&attachment).unwrap(), old_attachment);
    opts.key_alias = "不存在的别名".into();
    assert!(sign(&opts, |_| Ok(())).is_err());
    assert_eq!(fs::read(&output).unwrap(), signed_output);
    assert_eq!(fs::read(&attachment).unwrap(), old_attachment);
    opts.key_alias = "含,引号\"😀".into();
    opts.signing_versions.v4 = false;
    sign(&opts, |_| Ok(())).unwrap();
    assert!(!attachment.exists(), "关闭 V4 后不保留不匹配的旧附件");
    opts.output_path = None;
    sign(&opts, |_| Ok(())).unwrap();
    independently_verify(root, &stores.java, &input, &stores.fingerprint);
    assert_eq!(contents(&input), content);
    let signed = fs::read(&input).unwrap();
    opts.keystore_password = "wrong-test-password".into();
    assert!(sign(&opts, |_| Ok(())).is_err());
    assert_eq!(fs::read(&input).unwrap(), signed);
    assert_eq!(fs::read(&store).unwrap(), before_store);
    let bad_jar = root.join("bad-signer.jar");
    fs::write(&bad_jar, b"invalid-jar").unwrap();
    assert!(
        crate::apk_inspect::check_apk_default(&input, Some(&bad_jar)).is_err(),
        "坏工具不能伪装成未签名"
    );
    assert!(
        !crate::apk_inspect::check_apk_default(
            &super::signing_fixture::input(root, "未签名.apk"),
            opts.apksigner_path.as_deref()
        )
        .unwrap()
        .is_signed
    );
    assert_ne!(signed, original);
    assert!(
        walkdir::WalkDir::new(root)
            .into_iter()
            .any(|entry| entry.unwrap().file_name() == "aligned.apk"),
        "失败材料不得自动删除"
    );
    println!("普通默认签名：中文及补充字符路径／Alias、中文密码、指定输出覆盖、V4 副产物、同卷原地签名、独立验签／SDK 对齐、错误密码保留及坏工具不冒充未签名均通过");
}

#[test]
fn 普通签名准备取消不选择工具或创建输出() {
    let opts = SignOptions {
        apk_path: PathBuf::from("不存在.apk"),
        output_path: None,
        keystore_path: PathBuf::from("不存在.jks"),
        key_alias: String::new(),
        keystore_password: INITIAL.into(),
        key_password: INITIAL.into(),
        apksigner_path: None,
        keystore_type: KeystoreType::Jks,
        signing_versions: SigningVersions::default(),
    };
    let mut stages = vec![];
    assert!(sign_apk_default(&opts, |stage| {
        stages.push(stage);
        Err("开始前取消".into())
    })
    .is_err());
    assert_eq!(stages, [SigningProgressStep::Prepare]);
}

use super::*;

#[test]
fn 两条签名链路共用参数且密码不进入参数或文件请求() {
    let options = SignOptions {
        apk_path: PathBuf::from("中文 输入.apk"),
        output_path: None,
        keystore_path: PathBuf::from("中文 证书.jks"),
        key_alias: "中文Release".into(),
        keystore_password: "隔离存储口令456".into(),
        key_password: "隔离密钥口令789".into(),
        apksigner_path: None,
        keystore_type: KeystoreType::Jks,
        signing_versions: SigningVersions {
            v4: true,
            ..Default::default()
        },
    };
    let args = arguments(
        &options,
        &options.keystore_path,
        &options.apk_path,
        Path::new("中文 输出.apk"),
    )
    .unwrap();
    assert!(args
        .windows(2)
        .any(|pair| pair == ["--ks-key-alias", "中文Release"]));
    assert!(
        args.windows(2)
            .any(|pair| pair == ["--v4-signing-enabled", "true"]),
        "普通签名 V4 参数不能被适配悄悄删除"
    );
    assert_eq!(args.last().unwrap(), "中文 输入.apk");
    for secret in [&options.keystore_password, &options.key_password] {
        assert!(!args.iter().any(|arg| arg.contains(secret)));
    }
    assert!(args
        .iter()
        .any(|arg| arg == &format!("env:{}", crate::java_process::KS_PASS_ENV)));
    assert!(args
        .iter()
        .any(|arg| arg == &format!("env:{}", crate::java_process::KEY_PASS_ENV)));
}

use super::*;

#[test]
fn 普通共享加固按字段覆盖且不保存本次设置() {
    let temp = tempfile::tempdir().unwrap();
    let paths = ApplicationPaths::isolated(temp.path()).unwrap();
    let store = ConfigStore::new(paths.config_file());
    store.update_protect_defaults(serde_json::from_value(serde_json::json!({
        "runtime_mode":"android_api19", "environment_policy":"strict", "sign_after_protect":false,
    })).unwrap()).unwrap();
    let before = std::fs::read(paths.config_file()).unwrap();
    let config: CliConfig = toml::from_str("schema_version=1\n[protect]\ninput='input.apk'\noutput='output.apk'\nenvironment_policy='compatible'\n").unwrap();
    let result = config
        .resolve_protect(
            ProtectArgs {
                runtime_profile: Some("standard".into()),
                ..Default::default()
            },
            Some(temp.path()),
        )
        .unwrap();
    assert_eq!(
        result.runtime_profile,
        Some(shield_core::RuntimeProfile::Standard)
    );
    assert!(matches!(
        result.environment_policy,
        EnvironmentPolicyArg::Compatible
    ));
    assert!(result.signing.is_none());
    let inherited = config
        .resolve_protect(ProtectArgs::default(), Some(temp.path()))
        .unwrap();
    assert_eq!(
        inherited.runtime_profile,
        Some(shield_core::RuntimeProfile::AndroidApi19)
    );
    assert_eq!(std::fs::read(paths.config_file()).unwrap(), before);
}

#[test]
fn 旧受控合并拒绝新字段而不是读取共享状态() {
    let config: CliConfig = toml::from_str("schema_version=1\n[protect]\nuse_shared_settings=true\ninput='input.apk'\noutput='output.apk'\n").unwrap();
    assert!(config.merge_protect(ProtectArgs::default()).is_err());
    let old: CliConfig =
        toml::from_str("schema_version=1\n[protect]\ninput='input.apk'\noutput='output.apk'\n")
            .unwrap();
    let result = old.resolve_protect(ProtectArgs::default(), None).unwrap();
    assert!(result.runtime_profile.is_none() && result.signing.is_none());
}

#[test]
fn 本次拒绝共享设置不读取坏配置且关闭签名不读取证书() {
    let temp = tempfile::tempdir().unwrap();
    let paths = ApplicationPaths::isolated(temp.path()).unwrap();
    std::fs::create_dir_all(paths.config_file().parent().unwrap()).unwrap();
    std::fs::write(paths.config_file(), "[bad").unwrap();
    let config: CliConfig = toml::from_str("schema_version=1\n[protect]\nuse_shared_settings=true\ninput='input.apk'\noutput='output.apk'\n").unwrap();
    let result = config
        .resolve_protect(
            ProtectArgs {
                use_shared_settings: Some(false),
                sign_after_protect: Some(false),
                ..Default::default()
            },
            Some(temp.path()),
        )
        .unwrap();
    assert!(result.signing.is_none());
    assert!(!paths.certificate_database().exists());
    assert!(config
        .resolve_protect(ProtectArgs::default(), Some(temp.path()))
        .is_err());
}

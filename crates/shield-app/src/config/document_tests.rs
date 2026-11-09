use super::*;
use crate::config::{locking::ConfigLock, Setting};

fn patch(text: &str) -> ProtectDefaultsPatch {
    serde_json::from_str(text).unwrap()
}

#[test]
fn 只读缺失配置不会创建目录或锁文件() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("尚未创建");
    let store = ConfigStore::new(root.join("config.toml"));
    assert_eq!(
        store.read_protect_defaults().unwrap(),
        ProtectDefaults::default()
    );
    assert!(!root.exists());
}

#[test]
fn 字段更新保留其他入口状态和未知分区且空值明确清除() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("config.toml");
    fs::write(&path,"theme_mode='dark'\n[telemetry]\nenabled=false\nanonymous_id='fixture'\n[protect_defaults]\ncertificate_id='old'\nfuture_flag='keep'\n[future]\nvalue=42\n").unwrap();
    let store = ConfigStore::new(path.clone());
    store
        .update_protect_defaults(patch(
            r#"{"environment_policy":"strict","certificate_id":null}"#,
        ))
        .unwrap();
    let doc: Value = toml::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(doc["theme_mode"].as_str(), Some("dark"));
    assert_eq!(doc["telemetry"]["enabled"].as_bool(), Some(false));
    assert_eq!(
        doc["protect_defaults"]["future_flag"].as_str(),
        Some("keep")
    );
    assert_eq!(doc["future"]["value"].as_integer(), Some(42));
    assert!(doc["protect_defaults"].get("certificate_id").is_none());
    assert_eq!(store.read_protect_defaults().unwrap().certificate_id, None);
    store
        .update_protect_defaults(patch(r#"{"runtime_mode":"android_api19"}"#))
        .unwrap();
    assert_eq!(
        store.read_protect_defaults().unwrap().environment_policy,
        "strict"
    );
    assert_eq!(
        fs::read_dir(temp.path()).unwrap().count(),
        2,
        "只保留配置与稳定锁文件"
    );
}

#[test]
fn 初始化补默认但保留已有设置和未知字段() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("config.toml");
    fs::write(
        &path,
        "[protect_defaults]\nenvironment_policy='strict'\nextra=17\n[unknown]\nkey='preserve'\n",
    )
    .unwrap();
    let store = ConfigStore::new(path.clone());
    let result = store
        .initialize(&BusinessConfig::default(), |_| Ok(()))
        .unwrap();
    assert_eq!(result.protect_defaults.environment_policy, "strict");
    assert_eq!(
        store.read_protect_defaults().unwrap().runtime_mode,
        "standard"
    );
    let doc: Value = toml::from_str(&fs::read_to_string(path).unwrap()).unwrap();
    assert_eq!(doc["protect_defaults"]["extra"].as_integer(), Some(17));
    assert_eq!(doc["unknown"]["key"].as_str(), Some("preserve"));
}

#[test]
fn 损坏或较新配置不会被读取初始化或更新覆盖() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("config.toml");
    let store = ConfigStore::new(path.clone());
    for raw in [
        "[broken\nvalue=1",
        "schema_version=2\n",
        "schema_version='future'\n",
        "[protect_defaults]\nruntime_mode=17\n",
    ] {
        fs::write(&path, raw).unwrap();
        assert!(store.read_protect_defaults().is_err());
        assert!(store
            .initialize(&BusinessConfig::default(), |_| Ok(()))
            .is_err());
        assert!(store
            .update_protect_defaults(patch(r#"{"environment_policy":"strict"}"#))
            .is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), raw);
    }
}

#[test]
fn 非法更新不会先写入其他字段且已加载文件丢失不恢复旧缓存() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("config.toml");
    let store = ConfigStore::new(path.clone());
    store
        .update_protect_defaults(ProtectDefaultsPatch::default())
        .unwrap();
    let before = fs::read(&path).unwrap();
    let invalid = ProtectDefaultsPatch {
        runtime_mode: Setting::Set("unknown".into()),
        environment_policy: Setting::Set("strict".into()),
        ..Default::default()
    };
    assert_eq!(
        store.update_protect_defaults(invalid).unwrap_err(),
        ConfigError::InvalidValue
    );
    assert_eq!(before, fs::read(&path).unwrap());
    fs::remove_file(&path).unwrap();
    let existing = ConfigStore::existing(path.clone());
    assert!(existing.read_protect_defaults().is_err());
    assert!(existing
        .update_protect_defaults(ProtectDefaultsPatch::default())
        .is_err());
    assert!(!path.exists());
}

#[test]
fn 锁等待有界且锁文件在配置替换后保持同一对象() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("config.toml");
    let lock_path = temp.path().join("config.toml.lock");
    let held = ConfigLock::acquire(&lock_path, Duration::from_secs(1)).unwrap();
    let mut store = ConfigStore::new(path.clone());
    store.lock_timeout = Duration::from_millis(30);
    assert_eq!(
        store
            .update_protect_defaults(ProtectDefaultsPatch::default())
            .unwrap_err(),
        ConfigError::LockBusy
    );
    assert!(!path.exists());
    #[cfg(unix)]
    let identity = {
        use std::os::unix::fs::MetadataExt;
        fs::metadata(&lock_path).unwrap().ino()
    };
    drop(held);
    store
        .update_protect_defaults(ProtectDefaultsPatch::default())
        .unwrap();
    store
        .update_protect_defaults(patch(r#"{"environment_policy":"strict"}"#))
        .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        assert_eq!(fs::metadata(&lock_path).unwrap().ino(), identity);
    }
    assert!(lock_path.is_file());
}

#[cfg(unix)]
#[test]
fn 配置及锁文件权限收紧且不跟随文件链接() {
    use std::os::unix::fs::{symlink, PermissionsExt};
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("config.toml");
    let store = ConfigStore::new(path.clone());
    store
        .update_protect_defaults(ProtectDefaultsPatch::default())
        .unwrap();
    for file in [&path, &temp.path().join("config.toml.lock")] {
        assert_eq!(
            fs::metadata(file).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
    let linked = temp.path().join("linked.toml");
    symlink(&path, &linked).unwrap();
    let raw = fs::read(&path).unwrap();
    assert_eq!(
        ConfigStore::new(linked)
            .update_protect_defaults(ProtectDefaultsPatch::default())
            .unwrap_err(),
        ConfigError::InvalidPath
    );
    assert_eq!(fs::read(&path).unwrap(), raw);
}

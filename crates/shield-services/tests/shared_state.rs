//! 真实子进程验证配置锁与 SQLite 一致性；仅操作本次私有临时目录，不调用 Java。
use serde::{Deserialize, Serialize};
use shield_services::{
    certificates::{CertificateStoreState, CertificateUpsertInput},
    config::{ConfigStore, ProtectDefaultsPatch},
    paths::ApplicationPaths,
};
use std::{
    collections::BTreeMap,
    fs,
    io::Read,
    path::Path,
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default)]
struct Counters {
    counters: BTreeMap<String, u32>,
}

struct TestChild(Option<Child>);
impl TestChild {
    fn start(root: &Path, mode: &str, index: usize) -> Self {
        Self(Some(
            Command::new(std::env::current_exe().unwrap())
                .args(["--ignored", "--exact", "共享状态子进程夹具", "--nocapture"])
                .env("MOCIKA_SHARED_TEST_ROOT", root)
                .env("MOCIKA_SHARED_TEST_MODE", mode)
                .env("MOCIKA_SHARED_TEST_INDEX", index.to_string())
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap(),
        ))
    }
    fn finish(mut self) {
        let child = self.0.as_mut().unwrap();
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            if let Some(status) = child.try_wait().unwrap() {
                let mut errors = String::new();
                child
                    .stderr
                    .take()
                    .unwrap()
                    .take(8192)
                    .read_to_string(&mut errors)
                    .unwrap();
                assert!(status.success(), "隔离夹具子进程失败：{errors}");
                break;
            }
            assert!(Instant::now() < deadline, "隔离夹具子进程超时");
            std::thread::sleep(Duration::from_millis(10));
        }
        self.0.take();
    }
    fn stop(&mut self) {
        if let Some(child) = self.0.as_mut() {
            if child.try_wait().unwrap().is_none() {
                child.kill().unwrap();
            }
            child.wait().unwrap();
        }
    }
}
impl Drop for TestChild {
    fn drop(&mut self) {
        if let Some(mut child) = self.0.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

fn temp() -> tempfile::TempDir {
    tempfile::Builder::new()
        .prefix("mocika-shared-test-")
        .tempdir()
        .unwrap()
}
fn certificate(index: usize) -> CertificateUpsertInput {
    CertificateUpsertInput {
        id: None,
        name: format!("隔离测试证书{index}"),
        source_type: "external".into(),
        keystore_path: "fixture-not-keystore".into(),
        keystore_password: "synthetic-store-password".into(),
        key_alias: "release".into(),
        key_password: String::new(),
        ks_type: Some("PKCS12".into()),
        sign_v1: true,
        sign_v2: true,
        sign_v3: true,
        sign_v4: false,
        auto_sign_enabled: true,
        note: String::new(),
        set_as_default: true,
        copy_keystore_to_managed: false,
        managed_file_name: None,
    }
}

#[test]
#[ignore = "内部子进程夹具，只由本文件的隔离测试启动"]
fn 共享状态子进程夹具() {
    let root = std::path::PathBuf::from(std::env::var_os("MOCIKA_SHARED_TEST_ROOT").unwrap());
    assert!(
        root.is_dir()
            && root
                .file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("mocika-shared-test-")
    );
    let paths = ApplicationPaths::isolated(&root).unwrap();
    let index: usize = std::env::var("MOCIKA_SHARED_TEST_INDEX")
        .unwrap()
        .parse()
        .unwrap();
    match std::env::var("MOCIKA_SHARED_TEST_MODE").unwrap().as_str() {
        "config" => {
            let store = ConfigStore::existing(paths.config_file());
            for _ in 0..15 {
                store
                    .update(&Counters::default(), |value| {
                        *value.counters.entry(index.to_string()).or_default() += 1;
                        Ok(())
                    })
                    .unwrap();
            }
        }
        "certificate" => {
            let store = CertificateStoreState::initialize(
                paths.certificate_database(),
                paths.keystore_dir(),
            )
            .unwrap();
            for iteration in 0..5 {
                store
                    .save_certificate(
                        &certificate(index * 5 + iteration),
                        "fixture-not-keystore",
                        "release",
                        None,
                    )
                    .unwrap();
            }
        }
        "lock" => {
            let lock = fs::OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .truncate(false)
                .open(paths.config_file().with_extension("toml.lock"))
                .unwrap();
            lock.lock().unwrap();
            fs::write(root.join("ready"), b"locked").unwrap();
            std::thread::sleep(Duration::from_secs(15));
            drop(lock);
        }
        _ => panic!("未知测试模式"),
    }
}

#[test]
fn 多进程字段更新不丢失且原子替换期间读取始终完整() {
    let temp = temp();
    let paths = ApplicationPaths::isolated(temp.path()).unwrap();
    fs::create_dir_all(paths.config_file().parent().unwrap()).unwrap();
    fs::write(
        paths.config_file(),
        "theme_mode='dark'\n[telemetry]\nenabled=false\n[unknown]\nkeep='yes'\n",
    )
    .unwrap();
    let workers: Vec<_> = (0..4)
        .map(|index| TestChild::start(temp.path(), "config", index))
        .collect();
    for _ in 0..150 {
        let text = fs::read_to_string(paths.config_file()).unwrap();
        let _: toml::Value = toml::from_str(&text).expect("原子替换期间不能出现半个 TOML");
        std::thread::sleep(Duration::from_millis(1));
    }
    for worker in workers {
        worker.finish();
    }
    let value: Counters = ConfigStore::existing(paths.config_file())
        .read(&Counters::default())
        .unwrap();
    assert_eq!(value.counters.len(), 4);
    assert!(value.counters.values().all(|value| *value == 15));
    let text = fs::read_to_string(paths.config_file()).unwrap();
    let doc: toml::Value = toml::from_str(&text).unwrap();
    assert_eq!(doc["theme_mode"].as_str(), Some("dark"));
    assert_eq!(doc["telemetry"]["enabled"].as_bool(), Some(false));
    assert_eq!(doc["unknown"]["keep"].as_str(), Some("yes"));
}

#[test]
fn 持锁进程退出后其他入口可以继续更新() {
    let temp = temp();
    let paths = ApplicationPaths::isolated(temp.path()).unwrap();
    let store = ConfigStore::new(paths.config_file());
    store
        .update_protect_defaults(ProtectDefaultsPatch::default())
        .unwrap();
    let mut child = TestChild::start(temp.path(), "lock", 0);
    let until = Instant::now() + Duration::from_secs(5);
    while !temp.path().join("ready").is_file() {
        assert!(Instant::now() < until);
        std::thread::sleep(Duration::from_millis(10));
    }
    child.stop();
    let patch = serde_json::from_str(r#"{"environment_policy":"strict"}"#).unwrap();
    store.update_protect_defaults(patch).unwrap();
    assert_eq!(
        store.read_protect_defaults().unwrap().environment_policy,
        "strict"
    );
}

#[test]
fn 多进程证书写入及默认项事务不会互相丢失() {
    let temp = temp();
    let paths = ApplicationPaths::isolated(temp.path()).unwrap();
    let store =
        CertificateStoreState::initialize(paths.certificate_database(), paths.keystore_dir())
            .unwrap();
    let workers: Vec<_> = (0..4)
        .map(|index| TestChild::start(temp.path(), "certificate", index))
        .collect();
    for worker in workers {
        worker.finish();
    }
    let records = store.list_certificates().unwrap();
    assert_eq!(records.len(), 20);
    assert_eq!(records.iter().filter(|value| value.is_default).count(), 1);
    assert_eq!(
        records
            .iter()
            .map(|value| value.name.clone())
            .collect::<std::collections::BTreeSet<_>>()
            .len(),
        20
    );
}

#[test]
fn 其他进程增添证书和切换默认项不会作废所选绑定() {
    use shield_services::certificates::CertificateSelection;
    let temp = temp();
    let paths = ApplicationPaths::isolated(temp.path()).unwrap();
    let store =
        CertificateStoreState::initialize(paths.certificate_database(), paths.keystore_dir())
            .unwrap();
    let selected = store
        .save_certificate(&certificate(99), "fixture-not-keystore", "selected", None)
        .unwrap();
    let identity = store
        .capture_signing_identity(CertificateSelection::Default)
        .ok()
        .unwrap();
    let before = fs::read(paths.certificate_database()).unwrap();
    let workers: Vec<_> = (0..4)
        .map(|index| TestChild::start(temp.path(), "certificate", index))
        .collect();
    for worker in workers {
        worker.finish();
    }
    assert_ne!(fs::read(paths.certificate_database()).unwrap(), before);
    assert_eq!(store.list_certificates().unwrap().len(), 21);
    assert_eq!(identity.binding().certificate_id(), selected.id);
    assert!(store.check_certificate_binding(identity.binding()).is_ok());
    assert!(
        !store
            .get_certificate(&selected.id)
            .unwrap()
            .unwrap()
            .is_default
    );
}

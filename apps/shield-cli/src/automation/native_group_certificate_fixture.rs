//! 本次合成证书、隔离资料库和测试工作端；不读取真实资料或修改生产入口。
use super::*;
use shield_services::certificates::{
    CertificateStoreState, CertificateUpsertInput, CreateManagedCertificateInput,
};
use std::path::PathBuf;

pub(super) const PASSWORD: &str = "isolated-certificate-control-password";

pub(super) fn seed(directory: &Path) -> Vec<u8> {
    let path = directory.join("seed.jks");
    shield_core::keytool::generate_keystore_draft(
        &path,
        &shield_core::keytool::CreateKeystoreOptions {
            kind: "JKS",
            alias: "existing",
            keystore_password: PASSWORD,
            key_password: "",
            dname: "CN=ExistingFixture",
            validity_days: 2,
            key_size: 2048,
        },
    )
    .unwrap();
    fs::read(path).unwrap()
}

pub(super) struct Harness {
    _build: tempfile::TempDir,
    pub java: PathBuf,
    pub keytool: PathBuf,
    proxy: PathBuf,
    seed: Vec<u8>,
}

impl Harness {
    pub fn new() -> Self {
        let build = tempfile::tempdir().unwrap();
        let source = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/agent-signing/CertificateLifecycle.rs");
        let proxy = build
            .path()
            .join(format!("certificate-proxy{}", std::env::consts::EXE_SUFFIX));
        assert!(shield_core::utils::no_window_command("rustc")
            .arg(source)
            .args(["--edition=2021", "-o"])
            .arg(&proxy)
            .output()
            .unwrap()
            .status
            .success());
        let seed = seed(build.path());
        Self {
            java: shield_core::utils::find_java().unwrap(),
            keytool: shield_core::utils::find_keytool().unwrap(),
            seed,
            proxy,
            _build: build,
        }
    }

    pub fn case(&self) -> Case {
        let root = test_root();
        fs::remove_dir(root.path()).unwrap();
        private_directory(root.path());
        private_directory(&root.path().join("work"));
        fs::copy(
            &self.proxy,
            root.path().join(self.proxy.file_name().unwrap()),
        )
        .unwrap();
        let jar = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tools/java-adapter/build/java-adapter.jar");
        fs::copy(jar, root.path().join("adapter.jar")).unwrap();
        Case::new(root, &self.seed)
    }

    pub fn command(&self, root: &Path, phase: &str, mode: &str) -> Command {
        let mut command = fixture("certificate-worker", root);
        command
            .env("MOCIKA_TEST_REAL_JAVA", &self.java)
            .env("MOCIKA_TEST_REAL_KEYTOOL", &self.keytool)
            .env("MOCIKA_TEST_CERT_PHASE", phase)
            .env("MOCIKA_TEST_CERT_MODE", mode);
        command
    }
}

pub(super) struct Case {
    pub root: tempfile::TempDir,
    store: CertificateStoreState,
    state: serde_json::Value,
    database: Vec<u8>,
    original: Vec<u8>,
}
impl Case {
    pub fn new(root: tempfile::TempDir, seed: &[u8]) -> Self {
        let store = open_store(root.path(), true);
        let original = store.keystore_dir().join("existing.jks");
        fs::write(&original, seed).unwrap();
        let input = CertificateUpsertInput {
            id: None,
            name: "原有合成证书".into(),
            source_type: "external".into(),
            keystore_path: original.to_str().unwrap().into(),
            keystore_password: PASSWORD.into(),
            key_alias: "existing".into(),
            key_password: "".into(),
            ks_type: Some("JKS".into()),
            sign_v1: true,
            sign_v2: true,
            sign_v3: true,
            sign_v4: false,
            auto_sign_enabled: true,
            note: "本次新建的基线资料".into(),
            set_as_default: true,
            copy_keystore_to_managed: false,
            managed_file_name: None,
        };
        store
            .save_certificate(&input, original.to_str().unwrap(), "existing", None)
            .unwrap();
        let state = serde_json::to_value(store.list_certificates().unwrap()).unwrap();
        let database = fs::read(root.path().join("app/shield.db")).unwrap();
        Self {
            root,
            store,
            state,
            database,
            original: seed.to_owned(),
        }
    }
    pub fn unchanged(&self) {
        assert_eq!(
            serde_json::to_value(self.store.list_certificates().unwrap()).unwrap(),
            self.state
        );
        assert_eq!(
            fs::read(self.root.path().join("app/shield.db")).unwrap(),
            self.database
        );
        assert_eq!(
            fs::read(self.store.keystore_dir().join("existing.jks")).unwrap(),
            self.original
        );
        assert!(!self
            .store
            .keystore_dir()
            .join("new-certificate.jks")
            .exists());
        assert!(!self.root.path().join("worker-succeeded").exists());
    }
    pub fn material(&self, phase: &str) -> PathBuf {
        let name = if phase == "finish" {
            "final.keystore"
        } else {
            "generated.keystore"
        };
        fs::read_dir(self.root.path().join("work"))
            .unwrap()
            .map(|entry| entry.unwrap().path().join(name))
            .find(|path| path.is_file())
            .expect("目标阶段的真实材料必须存在")
    }
    pub fn retained(&self, phase: &str) {
        self.unchanged();
        assert!(self.root.path().join("adapter.jar").is_file());
        assert!(self.material(phase).is_file());
    }
}

pub(super) fn private_directory(path: &Path) {
    #[cfg(windows)]
    shield_core::windows_private::create_directory(path).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        fs::DirBuilder::new().mode(0o700).create(path).unwrap();
    }
}
pub(super) fn open_store(root: &Path, initialize: bool) -> CertificateStoreState {
    let db = root.join("app/shield.db");
    let keys = root.join("app/keystores");
    if initialize {
        CertificateStoreState::initialize(db, keys).unwrap()
    } else {
        CertificateStoreState::open_existing(db, keys).unwrap()
    }
}

pub(super) fn run_worker(root: &Path) {
    let store = open_store(root, false);
    let context =
        shield_core::JavaToolContext::new(&root.join("adapter.jar"), &root.join("work")).unwrap();
    let proxy = root.join(format!("certificate-proxy{}", std::env::consts::EXE_SUFFIX));
    let result = shield_services::certificates::create_managed_certificate_with_context(
        &store,
        CreateManagedCertificateInput {
            name: "待拒绝的新合成证书".into(),
            file_name: "new-certificate".into(),
            key_alias: "中文Release😀".into(),
            keystore_password: PASSWORD.into(),
            key_password: "".into(),
            ks_type: Some("JKS".into()),
            sign_v1: true,
            sign_v2: true,
            sign_v3: true,
            sign_v4: false,
            auto_sign_enabled: true,
            note: "".into(),
            set_as_default: true,
            dname: "CN=中文测试应用😀,O=隔离测试,C=CN".into(),
            validity_days: 2,
            key_size: 2048,
        },
        &context,
        &proxy,
        &proxy,
    );
    match result {
        Ok(_) => record(root, "worker-succeeded", b"unexpected"),
        Err(error) => {
            assert!(!error.message.contains(PASSWORD), "错误不能回显测试口令");
            let diagnostic = &error.diagnostic;
            record(
                root,
                "worker-failed",
                &serde_json::to_vec(&serde_json::json!({
                    "code": diagnostic.code, "tool": diagnostic.tool, "exit": diagnostic.exit_code,
                }))
                .unwrap(),
            );
            // 回执不是工作端退出或整组停止；保留租约与内层 Job，等待外层确认收尾。
            std::thread::sleep(Duration::from_secs(30));
            panic!("故障工作端必须由外层监督停止，不能自然返回");
        }
    }
}

pub(super) struct Denied {
    #[cfg(windows)]
    control: fs::File,
    #[cfg(unix)]
    path: PathBuf,
    #[cfg(unix)]
    permissions: fs::Permissions,
}
impl Denied {
    pub fn new(path: &Path) -> Self {
        #[cfg(windows)]
        {
            let control = permissions::control(path, false);
            permissions::set_acl(&control, "D:P");
            Self { control }
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_ne!(
                unsafe { libc::geteuid() },
                0,
                "特权进程不能用模式位证明拒绝访问"
            );
            let permissions = fs::metadata(path).unwrap().permissions();
            fs::set_permissions(path, fs::Permissions::from_mode(0o000)).unwrap();
            Self {
                path: path.to_path_buf(),
                permissions,
            }
        }
    }
    pub fn restore(self) {
        #[cfg(windows)]
        permissions::make_public(&self.control);
        #[cfg(unix)]
        fs::set_permissions(self.path, self.permissions).unwrap();
    }
}
#[cfg(windows)]
mod permissions {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/fixtures/windows_permissions.rs"
    ));
}

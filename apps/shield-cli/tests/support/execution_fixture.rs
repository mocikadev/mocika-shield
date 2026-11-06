//! 真实工具测试的临时材料；不读取业务证书、GUI 数据库或更改系统环境。
use sha2::{Digest, Sha256};
use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};

pub fn private(path: &Path, content: &[u8]) {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .unwrap();
    file.write_all(content).unwrap();
}

pub fn hash(path: &Path) -> [u8; 32] {
    let mut file = fs::File::open(path).unwrap();
    let mut digest = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let count = file.read(&mut buffer).unwrap();
        if count == 0 {
            break;
        }
        digest.update(&buffer[..count]);
    }
    digest.finalize().into()
}

pub struct Fixture {
    root: Option<tempfile::TempDir>,
    pub original: PathBuf,
    pub input: PathBuf,
    pub output: PathBuf,
    pub config: PathBuf,
    pub signer: PathBuf,
    original_hash: [u8; 32],
    input_hash: [u8; 32],
    active: AtomicBool,
}

impl Fixture {
    pub fn new() -> Self {
        use shield_core::utils::{
            find_apktool, find_java, find_keytool, find_runtime_resources, java_command,
        };
        let original = PathBuf::from(
            std::env::var_os("SHIELD_AGENT_TEST_APK").expect("请显式指定本机授权测试 APK"),
        );
        let original_hash = hash(&original);
        let root = tempfile::Builder::new()
            .permissions(fs::Permissions::from_mode(0o700))
            .tempdir()
            .unwrap();
        let keystore = root.path().join("临时测试证书.p12");
        let input = root.path().join("测试签名副本 中文.apk");
        let output = root.path().join("最终加固签名.apk");
        let repo = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()
            .unwrap();
        let signer = repo.join("tools/apksigner.jar");
        let created = shield_core::keytool::keytool_command(find_keytool().unwrap())
            .args([
                "-genkeypair",
                "-noprompt",
                "-storetype",
                "PKCS12",
                "-alias",
                "agentfixture",
                "-keyalg",
                "RSA",
                "-keysize",
                "2048",
                "-validity",
                "1",
                "-dname",
                "CN=本地自动化临时验证证书",
                "-storepass:env",
                "MOCIKA_SHIELD_KS_PASS",
                "-keypass:env",
                "MOCIKA_SHIELD_KS_PASS",
                "-keystore",
            ])
            .arg(&keystore)
            .env("MOCIKA_SHIELD_KS_PASS", Self::password())
            .output()
            .unwrap();
        assert!(created.status.success(), "测试证书创建失败");
        let prepared = java_command(find_java().unwrap())
            .arg("-jar")
            .arg(&signer)
            .args(["sign", "--ks"])
            .arg(&keystore)
            .args([
                "--ks-key-alias",
                "agentfixture",
                "--ks-pass",
                "env:MOCIKA_SHIELD_KS_PASS",
                "--key-pass",
                "env:MOCIKA_SHIELD_KS_PASS",
                "--v4-signing-enabled",
                "false",
                "--out",
            ])
            .arg(&input)
            .arg(&original)
            .env("MOCIKA_SHIELD_KS_PASS", Self::password())
            .output()
            .unwrap();
        assert!(
            prepared.status.success(),
            "准备临时签名副本失败，不会改签原始包"
        );
        let input_hash = hash(&input);
        let config = root.path().join("shield-cli.toml");
        let quote = |path: &Path| serde_json::to_string(path.to_str().unwrap()).unwrap();
        let content = format!("schema_version=1\n[protect]\napktool={}\napksigner={}\nresources={}\n[sign]\nkeystore={}\nkey_alias='agentfixture'\nkeystore_type='pkcs12'\nv1=true\nv2=true\nv3=true\nv4=false\n", quote(&find_apktool().unwrap()), quote(&signer), quote(&find_runtime_resources().unwrap()), quote(&keystore));
        private(&config, content.as_bytes());
        Self {
            root: Some(root),
            original,
            input,
            output,
            config,
            signer,
            original_hash,
            input_hash,
            active: AtomicBool::new(false),
        }
    }

    pub fn password() -> &'static str {
        "local-fixture-password-not-user-credential"
    }

    pub fn root(&self) -> &Path {
        self.root.as_ref().unwrap().path()
    }

    pub fn command(&self, program: &Path) -> Command {
        // 只有确认任务目录已回收才重新允许析构清理；失败断言不得抢删活动材料。
        self.active.store(true, Ordering::SeqCst);
        let mut command = shield_core::utils::no_window_command(program);
        command
            .env("MOCIKA_SHIELD_KS_PASS", Self::password())
            .env_remove("MOCIKA_SHIELD_KEY_PASS")
            .env_remove("JAVA_TOOL_OPTIONS")
            .env_remove("_JAVA_OPTIONS")
            .env_remove("JDK_JAVA_OPTIONS");
        command
    }

    pub fn assert_inputs_unchanged(&self) {
        assert_eq!(
            hash(&self.original),
            self.original_hash,
            "原始 APK 不得改动"
        );
        assert_eq!(hash(&self.input), self.input_hash, "执行输入副本不得改动");
    }

    pub fn assert_no_tasks(&self) {
        for entry in fs::read_dir(self.root()).unwrap() {
            assert!(
                !entry
                    .unwrap()
                    .file_name()
                    .to_string_lossy()
                    .starts_with(".mocika-"),
                "停止确认后不应残留任务目录"
            );
        }
        self.active.store(false, Ordering::SeqCst);
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        if self.active.load(Ordering::SeqCst) {
            if let Some(root) = self.root.take() {
                let _ = root.keep();
            }
        }
    }
}

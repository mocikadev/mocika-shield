//! 只负责新建隔离测试资料及原工具对照，不进入正式适配资源。
use super::*;
use crate::keytool::query_keystore_aliases;
use std::path::PathBuf;

pub(super) const INITIAL: &str = "isolated-readonly-bootstrap-password";
pub(super) const UNICODE: &str = "隔离口令一二三456";

pub(super) struct Stores {
    pub(super) root: tempfile::TempDir,
    pub(super) java: PathBuf,
    jar: PathBuf,
    pub(super) fingerprint: String,
}

impl Stores {
    pub(super) fn new() -> Self {
        let root = private_root();
        assert!(
            root.path().as_os_str().is_ascii(),
            "夹具编译及原工具对照需独立 ASCII 临时根"
        );
        let java = crate::utils::find_java().unwrap();
        let keytool = crate::utils::find_keytool().unwrap();
        let javac = keytool
            .parent()
            .unwrap()
            .join(format!("javac{}", std::env::consts::EXE_SUFFIX));
        let jar = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tools/java-adapter/build/java-adapter.jar");
        fs::copy(&jar, root.path().join("adapter.jar")).unwrap();
        fs::write(
            root.path().join("ReadOnlyStores.java"),
            include_str!("../../../../tests/fixtures/agent-signing/ReadOnlyStores.java"),
        )
        .unwrap();
        fs::write(
            root.path().join("CreatedCertificateProbe.java"),
            include_str!("../../../../tests/fixtures/agent-signing/CreatedCertificateProbe.java"),
        )
        .unwrap();
        assert!(
            crate::utils::no_window_command(javac)
                .current_dir(root.path())
                .args([
                    "-encoding",
                    "UTF-8",
                    "-source",
                    "8",
                    "-target",
                    "8",
                    "-cp",
                    "adapter.jar",
                    "-d",
                    ".",
                    "ReadOnlyStores.java",
                    "CreatedCertificateProbe.java"
                ])
                .output()
                .unwrap()
                .status
                .success(),
            "只读证书夹具编译失败"
        );
        assert!(
            crate::keytool::keytool_command(keytool)
                .current_dir(root.path())
                .args([
                    "-genkeypair",
                    "-keystore",
                    "bootstrap.jks",
                    "-alias",
                    "release",
                    "-storetype",
                    "JKS",
                    "-keyalg",
                    "RSA",
                    "-keysize",
                    "2048",
                    "-validity",
                    "2",
                    "-dname",
                    "CN=ReadonlyFixture",
                    "-storepass:env",
                    crate::java_process::KS_PASS_ENV,
                    "-keypass:env",
                    crate::java_process::KS_PASS_ENV
                ])
                .env(crate::java_process::KS_PASS_ENV, INITIAL)
                .output()
                .unwrap()
                .status
                .success(),
            "原工具合成证书生成失败"
        );
        let bootstrap = root.path().join("bootstrap.jks");
        assert_eq!(
            query_keystore_aliases(&bootstrap, INITIAL, Some("JKS")).unwrap(),
            ["release"]
        );
        let fingerprint =
            crate::extract_keystore_cert_fingerprint(&bootstrap, "release", INITIAL, Some("JKS"))
                .unwrap();
        Self {
            root,
            java,
            jar,
            fingerprint,
        }
    }

    pub(super) fn create(&self, kind: &str, variant: &str, password: &str) -> PathBuf {
        let directory = self.root.path().join(format!("中文材料-{kind}-{variant}"));
        create_workspace(&directory).unwrap();
        let path = directory.join("签名证书😀.keystore");
        let input_directory = private_io::directory(self.root.path()).unwrap();
        let bootstrap = self.root.path().join("bootstrap.jks");
        let values = [
            "fixture-create",
            bootstrap.to_str().unwrap(),
            path.to_str().unwrap(),
            kind,
            variant,
        ];
        let input = request::input(&input_directory, &request::encode(&values).unwrap()).unwrap();
        let classpath = if cfg!(windows) {
            "adapter.jar;."
        } else {
            "adapter.jar:."
        };
        let output = java_command(&self.java)
            .current_dir(self.root.path())
            .args(["-cp", classpath, "dev.mocika.shield.tools.ReadOnlyStores"])
            .env("SHIELD_READONLY_BOOTSTRAP_PASS", INITIAL)
            .env("SHIELD_READONLY_FIXTURE_PASS", password)
            .stdin(input)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "标准接口合成指定类型的证书夹具失败"
        );
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&output.stdout).unwrap(),
            json!({"created":true})
        );
        path
    }

    pub(super) fn context(&self) -> JavaToolContext {
        JavaToolContext::new(&self.jar, self.root.path()).unwrap()
    }

    pub(super) fn verify_created(
        &self,
        path: &Path,
        created: &crate::keytool::CreatedKeystore,
        options: &crate::keytool::CreateKeystoreOptions<'_>,
    ) {
        let directory = private_io::directory(self.root.path()).unwrap();
        let days = options.validity_days.to_string();
        let size = options.key_size.to_string();
        let values = [
            "verify-created",
            path.to_str().unwrap(),
            options.kind,
            &created.alias,
            options.dname,
            &days,
            &size,
        ];
        let input = request::input(&directory, &request::encode(&values).unwrap()).unwrap();
        let classpath = if cfg!(windows) {
            "adapter.jar;."
        } else {
            "adapter.jar:."
        };
        let output = java_command(&self.java)
            .current_dir(self.root.path())
            .args([
                "-cp",
                classpath,
                "dev.mocika.shield.tools.CreatedCertificateProbe",
            ])
            .env("SHIELD_READONLY_BOOTSTRAP_PASS", options.keystore_password)
            .env(
                "SHIELD_READONLY_FIXTURE_PASS",
                if options.key_password.is_empty() {
                    options.keystore_password
                } else {
                    options.key_password
                },
            )
            .stdin(input)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "独立标准接口未验证新建证书完整内容"
        );
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&output.stdout).unwrap(),
            json!({"verified":true})
        );
    }
}

pub(super) fn unchanged(path: &Path, expected: &[u8; 32]) {
    assert_eq!(
        &<[u8; 32]>::from(Sha256::digest(fs::read(path).unwrap())),
        expected,
        "只读调用不能改变原件"
    );
}

//! 签名监督的隔离资料、固定 JAR 门闩及工作端；不接真实资料库或公开执行契约。
use super::certificate_fixture::{self as baseline, Case, PASSWORD};
use super::*;
use shield_services::certificates::CertificateSelection;
use std::path::PathBuf;

pub(super) struct Harness {
    _build: tempfile::TempDir,
    pub java: PathBuf,
    proxy: PathBuf,
    gate: PathBuf,
    adapter: PathBuf,
    signer: PathBuf,
    seed: Vec<u8>,
    apk: Vec<u8>,
    fingerprint: String,
    certificate: [u8; 32],
    content: [u8; 32],
}

impl Harness {
    pub fn new() -> Self {
        let build = tempfile::tempdir().unwrap();
        let directory = shield_core::utils::strip_unc_prefix(build.path());
        assert!(
            directory.as_os_str().is_ascii(),
            "夹具编译使用独立 ASCII 临时根"
        );
        let repository = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let proxy = directory.join(format!("signing-proxy{}", std::env::consts::EXE_SUFFIX));
        assert!(shield_core::utils::no_window_command("rustc")
            .arg(repository.join("tests/fixtures/agent-signing/SigningLifecycle.rs"))
            .args(["--edition=2021", "-o"])
            .arg(&proxy)
            .output()
            .unwrap()
            .status
            .success());
        let java = shield_core::utils::find_java().unwrap();
        let keytool = shield_core::utils::find_keytool().unwrap();
        let javac = keytool
            .parent()
            .unwrap()
            .join(format!("javac{}", std::env::consts::EXE_SUFFIX));
        fs::write(
            directory.join("ApkSignerTool.java"),
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../tests/fixtures/agent-signing/SigningGate.java"
            )),
        )
        .unwrap();
        assert!(
            shield_core::utils::no_window_command(javac)
                .current_dir(&directory)
                .args([
                    "-encoding",
                    "UTF-8",
                    "-source",
                    "8",
                    "-target",
                    "8",
                    "-d",
                    ".",
                    "ApkSignerTool.java"
                ])
                .output()
                .unwrap()
                .status
                .success(),
            "签名 JAR 门闩编译失败"
        );
        let gate = directory.join("signing-gate.jar");
        let mut zip = zip::ZipWriter::new(fs::File::create(&gate).unwrap());
        zip.start_file(
            "META-INF/MANIFEST.MF",
            zip::write::SimpleFileOptions::default(),
        )
        .unwrap();
        zip.write_all(
            b"Manifest-Version: 1.0\r\nMain-Class: com.android.apksigner.ApkSignerTool\r\n\r\n",
        )
        .unwrap();
        let class = "com/android/apksigner/ApkSignerTool.class";
        zip.start_file(class, zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(&fs::read(directory.join(class)).unwrap())
            .unwrap();
        zip.finish().unwrap();
        let seed = baseline::seed(&directory);
        let fingerprint = shield_core::extract_keystore_cert_fingerprint(
            &directory.join("seed.jks"),
            "existing",
            PASSWORD,
            Some("JKS"),
        )
        .unwrap();
        let certificate: Vec<u8> = fingerprint
            .as_bytes()
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
            .collect();
        let input = PathBuf::from(std::env::var_os("SHIELD_TEST_APK").expect("指定自有真实 APK"));
        let content = shield_core::inspect_signing_input(&input, || Ok(()))
            .unwrap()
            .content_sha256;
        let mut zip = zip::ZipArchive::new(fs::File::open(&input).unwrap()).unwrap();
        let mut header = [0; 4];
        zip.by_name("AndroidManifest.xml")
            .unwrap()
            .read_exact(&mut header)
            .unwrap();
        assert_eq!(header, [3, 0, 8, 0], "不得用手工 XML 合成 ZIP 替代真实 APK");
        Self {
            _build: build,
            java,
            proxy,
            gate,
            seed,
            fingerprint,
            certificate: certificate.try_into().unwrap(),
            content,
            apk: fs::read(&input).unwrap(),
            adapter: repository.join("tools/java-adapter/build/java-adapter.jar"),
            signer: shield_core::utils::find_apksigner().unwrap(),
        }
    }

    pub fn case(&self) -> Case {
        let root = test_root();
        fs::remove_dir(root.path()).unwrap();
        baseline::private_directory(root.path());
        baseline::private_directory(&root.path().join("work"));
        for (source, name) in [
            (
                &self.proxy,
                format!("signing-proxy{}", std::env::consts::EXE_SUFFIX),
            ),
            (&self.gate, "signing-gate.jar".into()),
            (&self.adapter, "adapter.jar".into()),
            (&self.signer, "real-signer.jar".into()),
        ] {
            fs::copy(source, root.path().join(name)).unwrap();
        }
        fs::write(root.path().join("input.apk"), &self.apk).unwrap();
        fs::write(root.path().join("published.apk"), b"existing-public-output").unwrap();
        Case::new(root, &self.seed)
    }

    pub fn task(&self, seconds: u64) -> Task {
        Task::new_sign(
            Instant::now(),
            Duration::from_secs(seconds),
            PLAN,
            self.certificate,
            self.content,
        )
        .unwrap()
    }

    pub fn command(&self, root: &Path, phase: &str, mode: &str) -> Command {
        // Java 路径单独规范化，不改变 Rust 工作端或资料库的原路径上下文。
        let java_root = shield_core::utils::strip_unc_prefix(root);
        let mut command = fixture("signing-worker", root);
        command
            .env("MOCIKA_TEST_REAL_JAVA", &self.java)
            .env("MOCIKA_TEST_JAVA_ROOT", &java_root)
            .env("MOCIKA_TEST_REAL_SIGNER", java_root.join("real-signer.jar"))
            .env(
                "MOCIKA_TEST_SIGN_PROXY",
                java_root.join(format!("signing-proxy{}", std::env::consts::EXE_SUFFIX)),
            )
            .env("MOCIKA_TEST_SIGN_PHASE", phase)
            .env("MOCIKA_TEST_SIGN_MODE", mode);
        command
    }

    pub fn unchanged(&self, case: &Case) {
        case.unchanged();
        self.artifacts_unchanged(case.root.path());
    }

    pub fn artifacts_unchanged(&self, root: &Path) {
        assert_eq!(fs::read(root.join("input.apk")).unwrap(), self.apk);
        assert_eq!(
            fs::read(root.join("published.apk")).unwrap(),
            b"existing-public-output"
        );
        assert!(!root.join("signing-succeeded").exists());
    }

    pub fn retained(&self, root: &Path) {
        for name in ["adapter.jar", "signing-gate.jar", "real-signer.jar"] {
            assert!(root.join(name).is_file(), "停止证明前不能删除本次工具材料");
        }
        assert!(
            fs::read_dir(root.join("work")).unwrap().any(|entry| entry
                .unwrap()
                .path()
                .join("request.bin")
                .is_file()),
            "本次只读请求应仍留在借用根"
        );
    }

    pub fn verify_candidate(&self, root: &Path) {
        let root = shield_core::utils::strip_unc_prefix(root);
        let candidate = root.join("work/staged.apk");
        assert_eq!(
            shield_core::inspect_signing_input(&candidate, || Ok(()))
                .unwrap()
                .content_sha256,
            self.content
        );
        let result = shield_core::utils::java_command(&self.java)
            .arg("-jar")
            .arg(root.join("real-signer.jar"))
            .args(["verify", "--verbose", "--print-certs"])
            .arg(&candidate)
            .output()
            .unwrap();
        assert!(result.status.success(), "必须独立验证原工具确实完成了签名");
        let output = std::str::from_utf8(&result.stdout).unwrap();
        assert!(output.to_ascii_uppercase().contains(&self.fingerprint));
        for scheme in ["v1", "v2", "v3"] {
            assert!(output.lines().any(|line| line
                .starts_with(&format!("Verified using {scheme} scheme"))
                && line.ends_with(": true")));
        }
        let align = std::env::var_os("SHIELD_TEST_ZIPALIGN").expect("指定原 SDK 对齐校验工具");
        assert!(shield_core::utils::no_window_command(align)
            .args(["-c", "-P", "16", "4"])
            .arg(candidate)
            .output()
            .unwrap()
            .status
            .success());
    }
}

pub(super) fn run_worker(root: &Path) {
    let store = baseline::open_store(root, false);
    let identity = store
        .capture_signing_identity(CertificateSelection::Default)
        .unwrap();
    let cert = identity.certificate();
    let context =
        shield_core::JavaToolContext::new(&root.join("adapter.jar"), &root.join("work")).unwrap();
    let proxy = root.join(format!("signing-proxy{}", std::env::consts::EXE_SUFFIX));
    let output = root.join("work/staged.apk");
    let options = shield_core::SignOptions {
        apk_path: root.join("input.apk"),
        output_path: Some(output.clone()),
        keystore_path: PathBuf::from(&cert.keystore_path),
        key_alias: cert.key_alias.clone(),
        keystore_password: cert.keystore_password.clone(),
        key_password: cert.key_password.clone(),
        apksigner_path: Some(root.join("signing-gate.jar")),
        keystore_type: shield_core::KeystoreType::parse(&cert.ks_type),
        signing_versions: shield_core::SigningVersions {
            v1: cert.sign_v1,
            v2: cert.sign_v2,
            v3: cert.sign_v3,
            v4: false,
        },
    };
    let result = shield_core::sign_apk_with_context(&context, &proxy, &options, |_| Ok(()))
        .and_then(|_| {
            shield_core::extract_apk_cert_fingerprint_with_context(
                &context,
                &proxy,
                &output,
                &root.join("signing-gate.jar"),
            )
        });
    match result {
        Ok(_) => record(root, "signing-succeeded", b"unexpected"),
        Err(error) => {
            assert!(!format!("{error:#}").contains(PASSWORD), "错误不得回显口令");
            if std::env::var("MOCIKA_TEST_SIGN_MODE").unwrap() == "flood" {
                assert!(
                    error.to_string().contains("诊断已截断：true"),
                    "必须实际命中核心双流截断"
                );
            }
            let diagnostic = shield_core::diagnostic::Diagnostic::from_error(&error);
            record(root, "worker-failed", &serde_json::to_vec(&serde_json::json!({"code":diagnostic.code,"tool":diagnostic.tool,"exit":diagnostic.exit_code})).unwrap());
            // 保留租约及 Windows 内层 Job，不能用工作端自然返回替代外层停止证明。
            std::thread::sleep(Duration::from_secs(60));
            panic!("故障窗口必须由外层停止");
        }
    }
}

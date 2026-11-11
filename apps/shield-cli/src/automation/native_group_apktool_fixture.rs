//! 隔离 apktool／原生编译器及资料快照；不使用用户 APK 原件或真实证书库。
use super::certificate_fixture::{self as baseline, Case};
use super::*;
use std::path::PathBuf;

pub(super) struct Harness {
    _build: tempfile::TempDir,
    java: PathBuf,
    proxy: PathBuf,
    gate: PathBuf,
    tool: PathBuf,
    native: PathBuf,
    seed: Vec<u8>,
    apk: Vec<u8>,
}

impl Harness {
    pub fn new() -> Self {
        let build = tempfile::tempdir().unwrap();
        let directory = shield_core::utils::strip_unc_prefix(build.path());
        assert!(directory.as_os_str().is_ascii());
        let repository = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let java = shield_core::utils::find_java().unwrap();
        let javac = shield_core::utils::find_keytool()
            .unwrap()
            .parent()
            .unwrap()
            .join(format!("javac{}", std::env::consts::EXE_SUFFIX));
        fs::write(
            directory.join("Main.java"),
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../tests/fixtures/agent-signing/ApktoolGate.java"
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
                    "Main.java"
                ])
                .output()
                .unwrap()
                .status
                .success(),
            "apktool 测试门闩编译失败"
        );
        let gate = directory.join("apktool-gate.jar");
        let mut zip = zip::ZipWriter::new(fs::File::create(&gate).unwrap());
        zip.start_file(
            "META-INF/MANIFEST.MF",
            zip::write::SimpleFileOptions::default(),
        )
        .unwrap();
        zip.write_all(b"Manifest-Version: 1.0\r\nMain-Class: brut.apktool.Main\r\n\r\n")
            .unwrap();
        let class = "brut/apktool/Main.class";
        zip.start_file(class, zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(&fs::read(directory.join(class)).unwrap())
            .unwrap();
        zip.finish().unwrap();
        let proxy = directory.join(format!("apktool-native{}", std::env::consts::EXE_SUFFIX));
        assert!(shield_core::utils::no_window_command("rustc")
            .arg(repository.join("tests/fixtures/agent-signing/ApktoolNative.rs"))
            .args(["--edition=2021", "-D", "warnings", "-o"])
            .arg(&proxy)
            .output()
            .unwrap()
            .status
            .success());
        let tool = shield_core::utils::find_apktool().unwrap();
        let mut zip = zip::ZipArchive::new(fs::File::open(&tool).unwrap()).unwrap();
        let platform = if cfg!(windows) {
            "windows"
        } else if cfg!(target_os = "macos") {
            "macosx"
        } else {
            "linux"
        };
        let native = directory.join(format!("real-aapt2{}", std::env::consts::EXE_SUFFIX));
        let mut bytes = Vec::new();
        zip.by_name(&format!(
            "prebuilt/{platform}/aapt2{}",
            std::env::consts::EXE_SUFFIX
        ))
        .unwrap()
        .read_to_end(&mut bytes)
        .unwrap();
        fs::write(&native, bytes).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&native, fs::Permissions::from_mode(0o700)).unwrap();
        }
        let source = std::env::var_os("SHIELD_TEST_APK").expect("指定自有真实 APK");
        let apk = fs::read(source).unwrap();
        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(&apk)).unwrap();
        let mut header = [0; 4];
        zip.by_name("AndroidManifest.xml")
            .unwrap()
            .read_exact(&mut header)
            .unwrap();
        assert_eq!(header, [3, 0, 8, 0]);
        drop(zip);
        let seed = baseline::seed(&directory);
        Self {
            _build: build,
            java,
            proxy,
            gate,
            tool,
            native,
            seed,
            apk,
        }
    }

    pub fn case(&self) -> Case {
        let root = test_root();
        fs::remove_dir(root.path()).unwrap();
        baseline::private_directory(root.path());
        baseline::private_directory(&root.path().join("中文 工作"));
        let repository = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        for (source, name) in [
            (
                &self.proxy,
                format!("apktool-native{}", std::env::consts::EXE_SUFFIX),
            ),
            (
                &self.native,
                format!("real-aapt2{}", std::env::consts::EXE_SUFFIX),
            ),
            (&self.gate, "apktool-gate.jar".into()),
            (&self.tool, "real-apktool.jar".into()),
            (
                &repository.join("tools/java-adapter/build/java-adapter.jar"),
                "adapter.jar".into(),
            ),
        ] {
            fs::copy(source, root.path().join(name)).unwrap();
        }
        fs::write(root.path().join("中文 输入😀.apk"), &self.apk).unwrap();
        fs::write(root.path().join("published.apk"), b"existing-public-output").unwrap();
        Case::new(root, &self.seed)
    }

    pub fn command(&self, root: &Path, phase: &str, mode: &str) -> Command {
        let java_root = shield_core::utils::strip_unc_prefix(root);
        let mut command = fixture("apktool-worker", root);
        command
            .env("MOCIKA_TEST_REAL_JAVA", &self.java)
            .env("MOCIKA_TEST_JAVA_ROOT", &java_root)
            .env(
                "MOCIKA_TEST_REAL_APKTOOL",
                java_root.join("real-apktool.jar"),
            )
            .env(
                "MOCIKA_TEST_REAL_AAPT",
                java_root.join(format!("real-aapt2{}", std::env::consts::EXE_SUFFIX)),
            )
            .env(
                "MOCIKA_TEST_APKTOOL_PROXY",
                java_root.join(format!("apktool-native{}", std::env::consts::EXE_SUFFIX)),
            )
            .env("MOCIKA_TEST_APKTOOL_PHASE", phase)
            .env("MOCIKA_TEST_APKTOOL_MODE", mode);
        command
    }

    pub fn unchanged(&self, case: &Case) {
        case.unchanged();
        let root = case.root.path();
        assert_eq!(fs::read(root.join("中文 输入😀.apk")).unwrap(), self.apk);
        assert_eq!(
            fs::read(root.join("published.apk")).unwrap(),
            b"existing-public-output"
        );
        assert!(!root.join("apktool-succeeded").exists());
        assert!(root.join("real-apktool.jar").is_file() && root.join("adapter.jar").is_file());
    }

    pub fn retained(&self, root: &Path) {
        assert!(
            fs::read_dir(root.join("中文 工作")).unwrap().any(|entry| {
                let request = entry.unwrap().path().join("request.bin");
                request.is_file() && fs::read(request).unwrap().starts_with(b"MSJA")
            }),
            "停止前后均保留本次只读请求，不用输出 EOF 当清理证明"
        );
    }
}

pub(super) fn worker(root: &Path) {
    let work = root.join("中文 工作");
    let context = shield_core::JavaToolContext::new(&root.join("adapter.jar"), &work).unwrap();
    let java = PathBuf::from(std::env::var_os("MOCIKA_TEST_REAL_JAVA").unwrap());
    let result = context
        .decode_apk(
            &java,
            &root.join("apktool-gate.jar"),
            &root.join("中文 输入😀.apk"),
        )
        .and_then(|project| {
            fs::create_dir_all(project.directory().join("assets")).unwrap();
            fs::write(
                project.directory().join("assets/adapter-native-check.txt"),
                "真实重建内容",
            )
            .unwrap();
            project.rebuild()
        });
    if let Err(error) = result {
        let diagnostic = shield_core::diagnostic::Diagnostic::from_error(&error);
        record(root, "apktool-failed", &serde_json::to_vec(&serde_json::json!({"code":diagnostic.code,"tool":diagnostic.tool,"exit":diagnostic.exit_code,"truncated":error.to_string().contains("诊断已截断：true")})).unwrap());
        // 保持工作端租约／内层 Job，失败回执不能触发自动清场替代外层停止证明。
        std::thread::sleep(Duration::from_secs(60));
    } else {
        record(root, "apktool-succeeded", b"unexpected");
    }
}

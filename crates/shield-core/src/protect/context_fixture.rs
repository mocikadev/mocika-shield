//! 完整加固专项的自有 APK、临时证书和私有材料；不访问真实配置或证书库。
use super::*;
use crate::keytool::{create_keystore_draft_with_context, CreateKeystoreOptions};
use crate::{JavaToolContext, KeystoreType, SignOptions, SigningVersions};
use std::{io::Read, path::Path};

pub(super) struct Fixture {
    pub root: tempfile::TempDir,
    pub java: PathBuf,
    pub apktool: PathBuf,
    pub signer: PathBuf,
    pub store: PathBuf,
    pub fingerprint: String,
    alias: String,
}

pub(super) fn private_directory(path: &Path) {
    #[cfg(windows)]
    crate::windows_private::create_directory(path).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        fs::DirBuilder::new().mode(0o700).create(path).unwrap();
    }
}

impl Fixture {
    pub fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        #[cfg(windows)]
        {
            fs::remove_dir(root.path()).unwrap();
            private_directory(root.path());
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(root.path(), fs::Permissions::from_mode(0o700)).unwrap();
        }
        let java = crate::utils::find_java().unwrap();
        let tools = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tools");
        let apktool = tools.join("apktool_3.0.1.jar");
        let signer = tools.join("apksigner.jar");
        assert!(apktool.is_file() && signer.is_file());
        let workspace = root.path().join("create");
        private_directory(&workspace);
        let context = Self::context(&workspace);
        let store = root.path().join("中文证书😀.jks");
        let created = create_keystore_draft_with_context(
            &context,
            &java,
            &crate::utils::find_keytool().unwrap(),
            &store,
            &CreateKeystoreOptions {
                kind: "JKS",
                alias: "中文Release😀",
                keystore_password: "中文测试口令123",
                key_password: "独立密钥口令456",
                dname: "CN=自有加固验证,O=临时测试,C=CN",
                validity_days: 2,
                key_size: 2048,
            },
        )
        .unwrap();
        Self {
            root,
            java,
            apktool,
            signer,
            store,
            fingerprint: created.fingerprint,
            alias: created.alias,
        }
    }

    pub fn context(workspace: &Path) -> JavaToolContext {
        let adapter = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tools/java-adapter/build/java-adapter.jar");
        JavaToolContext::new(&adapter, workspace).unwrap()
    }

    pub fn workspace(&self, name: &str) -> PathBuf {
        let directory = self.root.path().join(name);
        private_directory(&directory);
        directory
    }

    pub fn sign(&self, context: &JavaToolContext, input: &Path, output: &Path) {
        crate::sign_apk_with_context(
            context,
            &self.java,
            &SignOptions {
                apk_path: input.to_owned(),
                output_path: Some(output.to_owned()),
                keystore_path: self.store.clone(),
                key_alias: self.alias.clone(),
                keystore_password: "中文测试口令123".into(),
                key_password: "独立密钥口令456".into(),
                apksigner_path: Some(self.signer.clone()),
                keystore_type: KeystoreType::Jks,
                signing_versions: SigningVersions::default(),
            },
            |_| Ok(()),
        )
        .unwrap();
    }

    pub fn options(&self, input: &Path, resources: &Path, name: &str) -> ProtectOptions {
        ProtectOptions {
            input: input.to_owned(),
            output: self.root.path().join(name),
            resources_path: Some(resources.to_owned()),
            apktool_path: Some(self.apktool.clone()),
            apksigner_path: Some(self.signer.clone()),
            expected_output_cert_fingerprint: Some(self.fingerprint.clone()),
            excluded_abis: Vec::new(),
            environment_policy: EnvironmentPolicy::Compatible,
        }
    }

    pub fn prepare(&self, resources: &Path, extract: bool, api19: bool) -> PathBuf {
        let workspace = self.workspace(&format!("prepare-{extract}-{api19}"));
        let context = Self::context(&workspace);
        let source = std::env::var_os("SHIELD_TEST_PROTECT_APK").expect("指定自有真实 DEX APK");
        let project = context
            .decode_apk(&self.java, &self.apktool, Path::new(&source))
            .unwrap();
        let directory = project.directory();
        let manifest = directory.join("AndroidManifest.xml");
        let content = fs::read_to_string(&manifest).unwrap();
        assert!(!content.contains("android:extractNativeLibs="));
        fs::write(
            manifest,
            content.replace(
                "<application",
                &format!("<application android:extractNativeLibs=\"{extract}\""),
            ),
        )
        .unwrap();
        let metadata = directory.join("apktool.yml");
        let content = fs::read_to_string(&metadata).unwrap();
        assert!(content.contains("minSdkVersion: 19"));
        if !api19 {
            fs::write(
                metadata,
                content.replace("minSdkVersion: 19", "minSdkVersion: 21"),
            )
            .unwrap();
        }
        for abi in ["armeabi-v7a", "arm64-v8a", "x86", "x86_64"] {
            let target = directory.join(format!("lib/{abi}"));
            fs::create_dir_all(&target).unwrap();
            let bytes = entry(resources, &format!("lib/{abi}/libmocikashield.so"));
            assert_eq!(&bytes[..4], b"\x7fELF");
            fs::write(target.join("liboriginalfixture.so"), bytes).unwrap();
        }
        if api19 {
            let target = directory.join("lib/armeabi");
            fs::create_dir_all(&target).unwrap();
            fs::write(target.join("liblegacy.so"), "仅供排除验证，不是可执行库").unwrap();
        }
        fs::create_dir_all(directory.join("assets")).unwrap();
        fs::write(
            directory.join("assets/中文内容😀.bin"),
            "原业务资源不得丢失",
        )
        .unwrap();
        let rebuilt = project.rebuild().unwrap();
        let output = self
            .root
            .path()
            .join(format!("输入中文😀-{extract}-{api19}.apk"));
        self.sign(&context, &rebuilt, &output);
        output
    }
}

pub(super) fn entry(path: &Path, name: &str) -> Vec<u8> {
    let mut zip = zip::ZipArchive::new(fs::File::open(path).unwrap()).unwrap();
    let mut bytes = Vec::new();
    zip.by_name(name).unwrap().read_to_end(&mut bytes).unwrap();
    bytes
}

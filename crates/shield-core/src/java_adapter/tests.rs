use super::*;
use crate::utils::strip_unc_prefix;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{fs, io::Cursor};

fn private_root() -> tempfile::TempDir {
    #[cfg(windows)]
    let root = tempfile::tempdir().unwrap();
    #[cfg(unix)]
    let root = {
        use std::os::unix::fs::PermissionsExt;
        tempfile::Builder::new()
            .permissions(fs::Permissions::from_mode(0o700))
            .tempdir()
            .unwrap()
    };
    #[cfg(windows)]
    {
        fs::remove_dir(root.path()).unwrap();
        crate::windows_private::create_directory(root.path()).unwrap();
    }
    root
}

fn create_workspace(path: &Path) -> std::io::Result<()> {
    #[cfg(windows)]
    {
        crate::windows_private::create_directory(path)
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        fs::DirBuilder::new().mode(0o700).create(path)
    }
}

fn fixture(version: &str, defect: &str) -> Vec<u8> {
    // 只供结构负向测试，不是可执行 class；真实 JVM 验证另行显式运行。
    let class = b"\xca\xfe\xba\xbe\0\0\0\x34";
    let prefix = "dev/mocika/shield/tools/";
    let mut classes = vec![];
    for name in [
        "AdapterMain",
        "Request",
        "Reply",
        "CertificateReader",
        "CertificateCreator",
        "JarLauncher",
    ] {
        classes.push(json!({"name":format!("{prefix}{name}.class"), "bytes":8, "sha256":format!("{:x}", Sha256::digest(class))}));
    }
    if defect == "digest" {
        classes[0]["sha256"] = json!("0".repeat(64));
    }
    if defect == "duplicate" {
        classes.push(classes[0].clone());
    }
    let metadata = json!({"protocol_version":if defect == "protocol" { PROTOCOL - 1 } else { PROTOCOL }, "application_version":version,
        "main_class":"dev.mocika.shield.tools.AdapterMain",
        "sources":[{"name":format!("{prefix}AdapterMain.java"),"sha256":"0".repeat(64)}],"classes":classes});
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let protocol = if defect == "protocol" {
        PROTOCOL - 1
    } else {
        PROTOCOL
    };
    let manifest = format!("Manifest-Version: 1.0\r\nMain-Class: dev.mocika.shield.tools.AdapterMain\r\nImplementation-Version: {version}\r\nMocika-Adapter-Protocol: {protocol}\r\n\r\n");
    for (name, data) in [
        ("META-INF/MANIFEST.MF".to_owned(), manifest.into_bytes()),
        (
            "META-INF/adapter.json".to_owned(),
            serde_json::to_vec(&metadata).unwrap(),
        ),
        (format!("{prefix}AdapterMain.class"), class.to_vec()),
        (format!("{prefix}Request.class"), class.to_vec()),
        (format!("{prefix}Reply.class"), class.to_vec()),
        (format!("{prefix}CertificateReader.class"), class.to_vec()),
        (format!("{prefix}CertificateCreator.class"), class.to_vec()),
        (format!("{prefix}JarLauncher.class"), class.to_vec()),
    ] {
        zip.start_file(name, zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(&data).unwrap();
    }
    if defect == "extra" {
        zip.start_file("unexpected.txt", zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(b"extra").unwrap();
    }
    zip.finish().unwrap().into_inner()
}

#[test]
fn 请求总量及只读输入不覆盖已有文件() {
    assert!(request::encode(&[]).is_err());
    assert!(request::encode(&[""; 65]).is_err());
    assert_eq!(request::encode(&["中文"]).unwrap().len(), 22);
    assert!(request::encode(&[&"a".repeat(request::MAX - 16)]).is_ok());
    assert!(request::encode(&[&"a".repeat(request::MAX - 15)]).is_err());
    let root = private_root();
    let bytes = request::encode(&["hello"]).unwrap();
    let mut file = request::input(root.path(), &bytes).unwrap();
    assert!(file.write_all(b"changed").is_err());
    assert!(request::input(root.path(), b"changed").is_err());
    assert_eq!(fs::read(root.path().join("request.bin")).unwrap(), bytes);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(file.metadata().unwrap().permissions().mode() & 0o777, 0o600);
    }
}

#[test]
fn 资源版本摘要角色和工作目录异常在启动前拒绝() {
    let root = private_root();
    let jar = root.path().join("adapter.jar");
    for defect in ["version", "protocol", "digest", "duplicate", "extra"] {
        fs::write(
            &jar,
            fixture(
                if defect == "version" {
                    "0.0.0"
                } else {
                    env!("CARGO_PKG_VERSION")
                },
                defect,
            ),
        )
        .unwrap();
        assert!(JavaToolContext::new(&jar, root.path()).is_err());
    }
    fs::write(&jar, fixture(env!("CARGO_PKG_VERSION"), "")).unwrap();
    assert!(JavaToolContext::new(Path::new("relative.jar"), root.path()).is_err());
    assert!(JavaToolContext::new(&jar, &jar).is_err());
    let context = JavaToolContext::new(&jar, root.path()).unwrap();
    fs::write(&jar, b"changed").unwrap();
    assert!(context.prepare(&std::env::current_exe().unwrap()).is_err());
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 1);
}

#[test]
fn 工作根同名替换后拒绝准备且不触碰新目录() {
    let root = private_root();
    let jar = root.path().join("adapter.jar");
    fs::write(&jar, fixture(env!("CARGO_PKG_VERSION"), "")).unwrap();
    let workspace = root.path().join("workspace");
    create_workspace(&workspace).unwrap();
    let context = JavaToolContext::new(&jar, &workspace).unwrap();
    fs::rename(&workspace, root.path().join("moved")).unwrap();
    create_workspace(&workspace).unwrap();
    assert!(context.prepare(&std::env::current_exe().unwrap()).is_err());
    assert_eq!(fs::read_dir(&workspace).unwrap().count(), 0);
    drop(context);
    assert!(workspace.is_dir());
    assert!(root.path().join("moved").is_dir());
}

#[test]
fn 不可安全引导的补充字符目录在启动前拒绝() {
    let root = private_root();
    let workspace = root.path().join("补充😀");
    create_workspace(&workspace).unwrap();
    let jar = root.path().join("adapter.jar");
    fs::write(&jar, fixture(env!("CARGO_PKG_VERSION"), "")).unwrap();
    let context = JavaToolContext::new(&jar, &workspace).unwrap();
    assert!(context.prepare(&std::env::current_exe().unwrap()).is_err());
    assert_eq!(fs::read_dir(&workspace).unwrap().count(), 0);
}

#[cfg(unix)]
#[test]
fn 资源和工作根不接受符号链接() {
    let root = private_root();
    let jar = root.path().join("adapter.jar");
    fs::write(&jar, fixture(env!("CARGO_PKG_VERSION"), "")).unwrap();
    let link = root.path().join("link.jar");
    std::os::unix::fs::symlink(&jar, &link).unwrap();
    assert!(JavaToolContext::new(&link, root.path()).is_err());
    let link = root.path().join("directory");
    std::os::unix::fs::symlink(root.path(), &link).unwrap();
    assert!(JavaToolContext::new(&jar, &link).is_err());
}

#[path = "access_tests.rs"]
mod access;
#[path = "apktool_tests.rs"]
mod apktool_tests;
#[path = "certificate_tests.rs"]
mod certificate;
#[path = "certificate_fixture.rs"]
mod certificate_fixture;
#[path = "creation_tests.rs"]
mod creation;
#[path = "jar_tests.rs"]
mod jars;
#[path = "ordinary_apk_tests.rs"]
mod ordinary_apk_tests;
#[path = "runtime_tests.rs"]
mod runtime;
#[path = "signing_tests.rs"]
mod signing;
#[path = "signing_fixture.rs"]
mod signing_fixture;

#[test]
fn 仅apktool临时属性在引导前固定且业务参数不进入原生argv() {
    let root = private_root();
    let adapter = root.path().join("adapter.jar");
    fs::write(&adapter, fixture(env!("CARGO_PKG_VERSION"), "")).unwrap();
    let tool = root.path().join("中文 工具.jar");
    fs::write(&tool, "仅准备命令，不运行此夹具").unwrap();
    let context = JavaToolContext::new(&adapter, root.path()).unwrap();
    for (kind, temporary) in [
        (super::jars::JarTool::Apktool, true),
        (super::jars::JarTool::Apksigner, false),
    ] {
        let command = context
            .prepare_jar(
                &std::env::current_exe().unwrap(),
                kind,
                &tool,
                &["中文参数"],
            )
            .unwrap();
        let args: Vec<_> = command.get_args().collect();
        assert!(args.iter().all(|arg| arg.is_ascii()));
        let property = args.iter().position(|arg| *arg == "-Djava.io.tmpdir=.");
        assert_eq!(property.is_some(), temporary);
        if let Some(property) = property {
            assert!(property < args.iter().position(|arg| *arg == "-jar").unwrap());
        }
        assert!(command
            .get_current_dir()
            .unwrap()
            .join("request.bin")
            .is_file());
    }
}

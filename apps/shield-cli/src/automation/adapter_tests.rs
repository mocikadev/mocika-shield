//! 合成 JAR 只验证材料结构与身份，不执行字节码，不作为实际 Java 兼容证明。
use super::*;
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{Cursor, Write},
};

fn resource(version: &str) -> Vec<u8> {
    let class = b"\xca\xfe\xba\xbe\0\0\0\x34";
    let names = [
        "AdapterMain",
        "Request",
        "Reply",
        "CertificateReader",
        "CertificateCreator",
        "JarLauncher",
    ];
    let prefix = "dev/mocika/shield/tools/";
    let metadata = serde_json::json!({"protocol_version":4,"application_version":version,
        "main_class":"dev.mocika.shield.tools.AdapterMain",
        "sources":[{"name":format!("{prefix}AdapterMain.java"),"sha256":"0".repeat(64)}],
        "classes":names.iter().map(|name| serde_json::json!({"name":format!("{prefix}{name}.class"),"bytes":class.len(),"sha256":format!("{:x}",Sha256::digest(class))})).collect::<Vec<_>>()});
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, data) in [
        ("META-INF/MANIFEST.MF", format!("Manifest-Version: 1.0\r\nMain-Class: dev.mocika.shield.tools.AdapterMain\r\nImplementation-Version: {version}\r\nMocika-Adapter-Protocol: 4\r\n\r\n").into_bytes()),
        ("META-INF/adapter.json", serde_json::to_vec(&metadata).unwrap()),
    ] {
        zip.start_file(name, zip::write::SimpleFileOptions::default()).unwrap();
        zip.write_all(&data).unwrap();
    }
    for name in names {
        zip.start_file(
            format!("{prefix}{name}.class"),
            zip::write::SimpleFileOptions::default(),
        )
        .unwrap();
        zip.write_all(class).unwrap();
    }
    zip.finish().unwrap().into_inner()
}

#[test]
fn 错版或损坏资源不产生请求且绑定不能补猜() {
    for data in [resource("0.0.0"), b"invalid jar".to_vec()] {
        let root = tempfile::tempdir().unwrap();
        let work = root.path().join("private");
        super::super::task_files::private_directory(&work).unwrap();
        let path = root.path().join("adapter.jar");
        fs::write(&path, data).unwrap();
        let binding = Binding::capture(&path).ok().unwrap();
        assert!(Bound::new(&binding, &path, &work).is_err());
        assert_eq!(fs::read_dir(&work).unwrap().count(), 0);
        assert_eq!(require(None).is_ok(), !cfg!(windows));
        assert_eq!(require(Some(&binding)).is_ok(), cfg!(windows));
    }
}

#[test]
fn 源和副本同字节替换内容变化及父子选择不一致均拒绝() {
    for defect in 0..5 {
        let root = tempfile::tempdir().unwrap();
        let work = root.path().join("private");
        super::super::task_files::private_directory(&work).unwrap();
        let source = root.path().join("source.jar");
        let copy = work.join("copy.jar");
        let bytes = resource(env!("CARGO_PKG_VERSION"));
        fs::write(&source, &bytes).unwrap();
        fs::write(&copy, &bytes).unwrap();
        let binding = Binding::capture(&source).ok().unwrap();
        if defect == 4 {
            fs::write(&copy, resource("0.0.0")).unwrap();
            assert!(Bound::new(&binding, &copy, &work).is_err());
            continue;
        }
        let bound = Bound::new(&binding, &copy, &work).ok().unwrap();
        let target = if defect < 2 { &source } else { &copy };
        let replacement = root.path().join("replacement.jar");
        fs::write(
            &replacement,
            if defect % 2 == 0 {
                bytes
            } else {
                resource("0.0.0")
            },
        )
        .unwrap();
        fs::rename(replacement, target).unwrap();
        assert!(bound.recheck().is_err());
        assert!(work.is_dir(), "身份错误不得自动删除借用根");
    }
}

#[test]
fn 适配来源链接改向不可沿用旧绑定() {
    let root = tempfile::tempdir().unwrap();
    let first = root.path().join("first.jar");
    let second = root.path().join("second.jar");
    let link = root.path().join("chosen.jar");
    fs::write(&first, resource(env!("CARGO_PKG_VERSION"))).unwrap();
    fs::copy(&first, &second).unwrap();
    #[cfg(unix)]
    use std::os::unix::fs::symlink;
    #[cfg(windows)]
    use std::os::windows::fs::symlink_file as symlink;
    symlink(&first, &link).unwrap();
    let binding = Binding::capture(&link).ok().unwrap();
    fs::remove_file(&link).unwrap();
    symlink(&second, &link).unwrap();
    assert!(binding.recheck(&mut || Ok(())).is_err());
}

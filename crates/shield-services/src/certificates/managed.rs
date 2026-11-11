//! 托管文件先在私有目录生成，再原子无覆盖放入证书目录；不负责数据库提交。
use std::{
    fs,
    path::{Path, PathBuf},
};

pub(super) fn draft(base: &Path) -> Result<tempfile::TempDir, String> {
    let mut builder = tempfile::Builder::new();
    builder.prefix(".mocika-keystore-");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        builder.permissions(fs::Permissions::from_mode(0o700));
    }
    builder
        .tempdir_in(base)
        .map_err(|e| format!("创建证书暂存目录失败: {e}"))
}

/// 外层暂存所有者负责回收；只对本次新建的子目录设置私有边界。
pub(super) fn adapter_directory(base: &Path) -> Result<PathBuf, String> {
    let path = base.join("adapter-material");
    #[cfg(windows)]
    shield_core::windows_private::create_directory(&path)
        .map_err(|_| "创建私有证书材料目录失败".to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        fs::DirBuilder::new()
            .mode(0o700)
            .create(&path)
            .map_err(|_| "创建私有证书材料目录失败".to_string())?;
    }
    Ok(path)
}

pub(super) fn publish(source: &Path, base: &Path, name: &str) -> Result<PathBuf, String> {
    if Path::new(name).components().count() != 1
        || Path::new(name)
            .file_name()
            .is_none_or(|value| value != name)
    {
        return Err("托管证书文件名无效".into());
    }
    let meta = fs::symlink_metadata(source).map_err(|e| format!("读取暂存证书失败: {e}"))?;
    if !meta.is_file() {
        return Err("暂存证书必须是普通文件".into());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(source, fs::Permissions::from_mode(0o600))
            .map_err(|e| format!("收紧证书权限失败: {e}"))?;
    }
    let path = Path::new(name);
    let stem = path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("keystore");
    let ext = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("");
    for index in 1..1000 {
        let filename = match (index, ext.is_empty()) {
            (1, _) => name.to_owned(),
            (_, true) => format!("{stem}-{index}"),
            (_, false) => format!("{stem}-{index}.{ext}"),
        };
        let target = base.join(filename);
        // 两条路径在同一应用数据文件系统；AlreadyExists 只能换名，不能覆盖或修改原件。
        match fs::hard_link(source, &target) {
            Ok(()) => return Ok(target),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(format!("无覆盖保存托管证书失败: {error}")),
        }
    }
    Err("托管证书同名文件过多，请更换文件名".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn 多个同时创建的同名证书不会修改原文件或互相覆盖() {
        let temp = tempfile::tempdir().unwrap();
        let existing = temp.path().join("release.p12");
        fs::write(&existing, b"original").unwrap();
        let workers: Vec<_> = (0..8)
            .map(|index| {
                let base = temp.path().to_owned();
                std::thread::spawn(move || {
                    let stage = draft(&base).unwrap();
                    let source = stage.path().join("generated.p12");
                    fs::write(&source, format!("certificate-{index}")).unwrap();
                    let output = publish(&source, &base, "release.p12").unwrap();
                    (output, format!("certificate-{index}"))
                })
            })
            .collect();
        let mut paths = std::collections::HashSet::new();
        for worker in workers {
            let (path, expected) = worker.join().unwrap();
            assert_eq!(fs::read_to_string(&path).unwrap(), expected);
            assert!(paths.insert(path));
        }
        assert_eq!(fs::read(existing).unwrap(), b"original");
        assert_eq!(fs::read_dir(temp.path()).unwrap().count(), 9);
    }
    #[test]
    fn 非法路径不会跳出证书目录() {
        let temp = tempfile::tempdir().unwrap();
        let stage = draft(temp.path()).unwrap();
        let source = stage.path().join("generated.p12");
        fs::write(&source, b"fixture").unwrap();
        for name in ["../escape.p12", "/tmp/escape.p12", ".", "..", ""] {
            assert!(publish(&source, temp.path(), name).is_err());
        }
    }
}

//! 桌面共包资源的固定布局；不读取环境、用户数据或开发目录。
use anyhow::Result;
use std::path::{Path, PathBuf};

pub(crate) fn root(executable: &Path) -> Option<PathBuf> {
    let executable = crate::utils::strip_unc_prefix(executable);
    macos_root(&executable).or({
        #[cfg(target_os = "windows")]
        {
            windows_root(&executable)
        }
        #[cfg(target_os = "linux")]
        {
            linux_root(&executable)
        }
        #[cfg(not(any(target_os = "windows", target_os = "linux")))]
        None
    })
}

fn macos_root(executable: &Path) -> Option<PathBuf> {
    let directory = executable.parent()?;
    let contents = directory.parent()?;
    (directory.file_name()? == "MacOS"
        && contents.file_name()? == "Contents"
        && contents.parent()?.extension()? == "app")
        .then(|| contents.join("Resources"))
}

#[cfg(any(target_os = "windows", test))]
fn windows_root(executable: &Path) -> Option<PathBuf> {
    (executable.file_name()? == "mocika-shield-cli.exe")
        .then(|| executable.parent().unwrap().to_path_buf())
}

#[cfg(any(target_os = "linux", test))]
fn linux_root(executable: &Path) -> Option<PathBuf> {
    (executable.file_name()? == "mocika-shield-cli")
        .then(|| executable.parent().unwrap().join("../lib/MocikaShield"))
}

/// 已识别为桌面包的入口只使用同包资源，缺失或不是普通文件立即失败。
pub(crate) fn resource(relative: &str) -> Result<Option<PathBuf>> {
    let executable = std::env::current_exe()?;
    required(root(&executable), relative)
}

fn required(root: Option<PathBuf>, relative: &str) -> Result<Option<PathBuf>> {
    let Some(root) = root else { return Ok(None) };
    let path = crate::utils::strip_unc_prefix(&root.join(relative));
    if !std::fs::symlink_metadata(&path).is_ok_and(|metadata| metadata.is_file()) {
        anyhow::bail!("应用包缺少有效内置资源 {relative}，请重新安装完整应用包。");
    }
    Ok(Some(path))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 三平台布局固定且不取决于当前目录或环境() {
        let base = Path::new("/中文 空格");
        assert_eq!(
            macos_root(&base.join("测试.app/Contents/MacOS/shield")),
            Some(base.join("测试.app/Contents/Resources"))
        );
        assert_eq!(
            windows_root(&base.join("mocika-shield-cli.exe")),
            Some(base.to_path_buf())
        );
        for prefix in ["usr", "挂载点/usr"] {
            let path = base.join(prefix).join("bin/mocika-shield-cli");
            assert_eq!(
                linux_root(&path),
                Some(base.join(prefix).join("bin/../lib/MocikaShield"))
            );
        }
        assert!(macos_root(&base.join("not-app/Contents/MacOS/shield")).is_none());
        assert!(windows_root(&base.join("shield.exe")).is_none());
        assert!(linux_root(&base.join("bin/shield")).is_none());
    }

    #[test]
    fn 同包资源缺失或变成目录立即失败() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().to_path_buf();
        for relative in [
            "tools/apktool.jar",
            "tools/apksigner.jar",
            "tools/java-adapter.jar",
            "resources/resources.zip",
        ] {
            assert!(required(Some(root.clone()), relative).is_err());
            let path = root.join(relative);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, "仅验证布局").unwrap();
            assert_eq!(
                required(Some(root.clone()), relative).unwrap(),
                Some(path.clone())
            );
            std::fs::remove_file(&path).unwrap();
            std::fs::create_dir(&path).unwrap();
            assert!(required(Some(root.clone()), relative).is_err());
        }
        assert!(required(None, "tools/apktool.jar").unwrap().is_none());
    }
}

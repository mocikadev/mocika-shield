//! 正式适配资源的固定查找顺序；不启动工具、不持有计划或选择工作目录。
use anyhow::Result;
use std::path::{Path, PathBuf};

/// 从同包位置选择资源；首个已存在位置异常即拒绝，不联网或跨来源重试。
/// 仅调试源码构建允许使用仓库生成目录；选中后仍须由上下文核对版本和协议。
pub fn find_java_adapter() -> Result<PathBuf> {
    let executable = std::env::current_exe()?;
    select(
        &executable,
        cfg!(debug_assertions).then(|| {
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../tools/java-adapter/build/java-adapter.jar")
        }),
    )
}

fn select(executable: &Path, development: Option<PathBuf>) -> Result<PathBuf> {
    let parent = executable.parent().ok_or_else(super::invalid)?;
    // 所有正式桌面共包入口使用同包资源，不借开发目录掩盖残缺安装。
    if let Some(root) = crate::bundle_resources::root(executable) {
        let path = root.join("tools/java-adapter.jar");
        return select_from(&[path], None);
    }
    select_from(
        &[
            parent.join("tools/java-adapter.jar"),
            parent.join("../tools/java-adapter.jar"),
            parent.join("../lib/java-adapter.jar"),
        ],
        development,
    )
}

fn select_from(packaged: &[PathBuf], development: Option<PathBuf>) -> Result<PathBuf> {
    for path in packaged.iter().chain(development.iter()) {
        match std::fs::symlink_metadata(path) {
            Ok(metadata) if metadata.is_file() => return Ok(crate::utils::strip_unc_prefix(path)),
            Ok(_) => return Err(super::invalid()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err(super::invalid()),
        }
    }
    Err(super::invalid())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn 同包优先且坏位置不回退开发资源() {
        let root = tempfile::tempdir().unwrap();
        let executable = root.path().join("bin/shield");
        let development = root.path().join("development.jar");
        let packaged = root.path().join("tools/java-adapter.jar");
        fs::create_dir_all(executable.parent().unwrap()).unwrap();
        fs::write(&development, "仅验证选择顺序").unwrap();
        assert_eq!(
            select(&executable, Some(development.clone())).unwrap(),
            development
        );
        fs::create_dir_all(packaged.parent().unwrap()).unwrap();
        fs::write(&packaged, "选中后由上下文核验内容").unwrap();
        assert_eq!(
            select(&executable, Some(development.clone())).unwrap(),
            crate::utils::strip_unc_prefix(
                &executable
                    .parent()
                    .unwrap()
                    .join("../tools/java-adapter.jar")
            )
        );
        fs::remove_file(&packaged).unwrap();
        fs::create_dir(&packaged).unwrap();
        assert!(select(&executable, Some(development)).is_err());
    }

    #[test]
    fn 图形入口与应用包不借开发目录掩盖缺失() {
        let root = tempfile::tempdir().unwrap();
        let gui = root.path().join("gui/mocika-shield.exe");
        let packaged = root.path().join("gui/tools/java-adapter.jar");
        fs::create_dir_all(packaged.parent().unwrap()).unwrap();
        fs::write(&packaged, "同包图形入口").unwrap();
        assert_eq!(select(&gui, None).unwrap(), packaged);
        let app = root.path().join("测试.app/Contents/MacOS/shield");
        fs::create_dir_all(app.parent().unwrap()).unwrap();
        assert!(select(&app, Some(packaged)).is_err());
        let bundled = root
            .path()
            .join("测试.app/Contents/Resources/tools/java-adapter.jar");
        fs::create_dir_all(bundled.parent().unwrap()).unwrap();
        fs::write(&bundled, "同包命令入口").unwrap();
        assert_eq!(select(&app, None).unwrap(), bundled);
    }

    #[test]
    #[cfg(any(target_os = "linux", windows))]
    fn 桌面共包命令不借开发资源掩盖缺失() {
        let root = tempfile::tempdir().unwrap();
        #[cfg(windows)]
        let executable = root.path().join("安装路径/mocika-shield-cli.exe");
        #[cfg(target_os = "linux")]
        let executable = root.path().join("挂载路径/usr/bin/mocika-shield-cli");
        let development = root.path().join("development.jar");
        std::fs::write(&development, "不能回退到此文件").unwrap();
        assert!(select(&executable, Some(development.clone())).is_err());
        let bundled = crate::bundle_resources::root(&executable)
            .unwrap()
            .join("tools/java-adapter.jar");
        // Linux 的 .. 路径解析需要实际 bin 目录，模拟真实安装布局。
        std::fs::create_dir_all(executable.parent().unwrap()).unwrap();
        std::fs::create_dir_all(bundled.parent().unwrap()).unwrap();
        std::fs::write(&bundled, "由后续正式上下文验证内容").unwrap();
        assert_eq!(
            select(&executable, Some(development)).unwrap(),
            crate::utils::strip_unc_prefix(&bundled)
        );
    }
}

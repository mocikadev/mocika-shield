//! 统一桌面入口的数据身份；保持原应用标识，不因入口名称改变旧密文路径。
use std::path::{Path, PathBuf};

pub const APPLICATION_ID: &str = "dev.mocika.shield-gui";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ApplicationPaths {
    config_dir: PathBuf,
    data_dir: PathBuf,
}
impl ApplicationPaths {
    /// 与现有 Tauri 的 dirs::config_dir/data_dir 加应用标识算法一致；不创建目录。
    pub fn current_user() -> Result<Self, &'static str> {
        Self::from_directories(
            dirs::config_dir()
                .ok_or("无法定位应用配置目录")?
                .join(APPLICATION_ID),
            dirs::data_dir()
                .ok_or("无法定位应用数据目录")?
                .join(APPLICATION_ID),
        )
    }
    pub fn from_directories(config_dir: PathBuf, data_dir: PathBuf) -> Result<Self, &'static str> {
        for path in [&config_dir, &data_dir] {
            if !path.is_absolute()
                || path.parent().is_none()
                || path
                    .components()
                    .any(|part| matches!(part, std::path::Component::ParentDir))
            {
                return Err("应用目录必须是明确的绝对子目录");
            }
        }
        // 只简化 Windows 扩展路径，不 canonicalize 或重定位原密码派生所用路径。
        Ok(Self {
            config_dir: dunce::simplified(&config_dir).to_path_buf(),
            data_dir: dunce::simplified(&data_dir).to_path_buf(),
        })
    }
    /// 测试／CI 的显式隔离上下文；不回退当前用户数据。
    pub fn isolated(root: &Path) -> Result<Self, &'static str> {
        if !root.is_absolute()
            || root.parent().is_none()
            || root
                .components()
                .any(|part| matches!(part, std::path::Component::ParentDir))
        {
            return Err("隔离目录无效");
        }
        Self::from_directories(root.join("config"), root.join("data"))
    }
    pub fn config_file(&self) -> PathBuf {
        self.config_dir.join("config.toml")
    }
    pub fn certificate_database(&self) -> PathBuf {
        self.data_dir.join("shield.db")
    }
    pub fn keystore_dir(&self) -> PathBuf {
        self.data_dir.join("keystores")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn 标识与打包一致且默认定位只计算路径() {
        let bundle: serde_json::Value = serde_json::from_str(include_str!(
            "../../../apps/shield-gui/src-tauri/tauri.conf.json"
        ))
        .unwrap();
        assert_eq!(bundle["identifier"], APPLICATION_ID);
        let actual = ApplicationPaths::current_user().unwrap();
        let expected = ApplicationPaths::from_directories(
            dirs::config_dir().unwrap().join(APPLICATION_ID),
            dirs::data_dir().unwrap().join(APPLICATION_ID),
        )
        .unwrap();
        assert_eq!(actual, expected);
    }
    #[test]
    fn 隔离上下文不创建或借用真实目录() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("中文 空格");
        let paths = ApplicationPaths::isolated(&root).unwrap();
        assert_eq!(paths.config_file(), root.join("config/config.toml"));
        assert_eq!(paths.certificate_database(), root.join("data/shield.db"));
        assert!(!root.exists());
        assert!(ApplicationPaths::isolated(Path::new("relative")).is_err());
        assert!(ApplicationPaths::isolated(&temp.path().join("../outside")).is_err());
    }
}

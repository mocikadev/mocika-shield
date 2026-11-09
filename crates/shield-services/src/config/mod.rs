//! 配置文件事务和共享业务默认值，不读取 GUI 数据类型或宿主授权。
mod document;
mod locking;
mod models;
pub use document::ConfigStore;
pub use models::{normalize_protect_defaults, ProtectDefaults, ProtectDefaultsPatch, Setting};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConfigError {
    InvalidPath,
    ReadFailed,
    TooLarge,
    InvalidDocument,
    UnsupportedVersion,
    LockBusy,
    LockFailed,
    WriteFailed,
    InvalidValue,
}
impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::InvalidPath => "配置路径无效，不能使用目录或符号链接",
            Self::ReadFailed => "读取配置失败，不使用缓存覆盖原文件",
            Self::TooLarge => "配置文件超过安全大小上限",
            Self::InvalidDocument => "配置格式无效，请检查原文件；不会重置或覆盖",
            Self::UnsupportedVersion => "配置格式版本不受支持，不能覆盖较新版本",
            Self::LockBusy => "配置正在被其他入口更新，请稍后再试",
            Self::LockFailed => "无法取得配置更新锁，不会写入文件",
            Self::WriteFailed => "配置未能安全写入，请检查目录和权限",
            Self::InvalidValue => "业务设置值无效，请核对模式与策略",
        })
    }
}
impl std::error::Error for ConfigError {}

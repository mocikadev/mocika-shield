#![allow(dead_code)]

use thiserror::Error;

#[derive(Error, Debug)]
pub enum ShieldError {
    #[error("工具未找到: {0}")]
    ToolNotFound(String),

    #[error("命令执行失败: {0}")]
    CommandFailed(String),

    #[error("文件不存在: {0}")]
    FileNotFound(String),

    #[error("配置错误: {0}")]
    ConfigError(String),

    #[error("APK处理失败: {0}")]
    ApkError(String),

    #[error("操作已取消")]
    Cancelled,

    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

pub type Result<T> = std::result::Result<T, ShieldError>;

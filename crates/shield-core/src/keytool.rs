//! keytool 子进程的文本协议：固定编码和语言，不依赖桌面会话的区域设置。
use std::ffi::OsStr;
use std::process::Command;

/// 保留 Windows 隐藏控制台行为，仅配置本次子进程，不修改系统 Java 环境。
pub fn keytool_command(path: impl AsRef<OsStr>) -> Command {
    crate::java_process::keytool_command(path)
}

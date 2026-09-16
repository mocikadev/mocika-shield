//! keytool 子进程的文本协议：固定编码和语言，不依赖桌面会话的区域设置。
use crate::utils::no_window_command;
use std::ffi::OsStr;
use std::process::Command;

/// 保留 Windows 隐藏控制台行为，仅配置本次子进程，不修改系统 Java 环境。
pub fn keytool_command(path: impl AsRef<OsStr>) -> Command {
    let mut command = no_window_command(path);
    command.args([
        "-J-Dfile.encoding=UTF-8",
        // Java 8/17 的标准流与新 JDK 的标准流分别显式约定，未知属性不会改变工具参数。
        "-J-Dsun.stdout.encoding=UTF-8",
        "-J-Dsun.stderr.encoding=UTF-8",
        "-J-Dstdout.encoding=UTF-8",
        "-J-Dstderr.encoding=UTF-8",
        "-J-Duser.language=en",
        "-J-Duser.country=US",
    ]);
    command
}

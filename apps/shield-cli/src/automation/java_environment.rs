//! 本次工作进程的 Java 用户目录与临时目录；不修改父进程或工具发现规则。
use super::CheckError;
use std::{path::Path, process::Command};

pub(super) fn apply(command: &mut Command, root: &Path) -> Result<(), CheckError> {
    let root = shield_core::utils::strip_unc_prefix(root);
    #[cfg(windows)]
    let root = {
        let path = checked(&root)?;
        if path.is_ascii() {
            root
        } else {
            windows::ascii_directory(&root)?
        }
    };
    let path = checked(&root)?;
    // HotSpot 按引号解析环境选项，不是 shell／Windows argv 的反斜杠转义规则。
    let options = format!("\"-Duser.home={path}\" \"-Djava.io.tmpdir={path}\"");
    // Java 8 的环境选项缓冲区为 1024 字节；超长时会静默忽略，必须提前拒绝。
    if options.len() >= 1024 {
        return Err(invalid());
    }
    command
        .env("TMPDIR", &root)
        .env("TMP", &root)
        .env("TEMP", &root)
        .env("JAVA_TOOL_OPTIONS", options);
    Ok(())
}

fn checked(root: &Path) -> Result<&str, CheckError> {
    #[cfg(windows)]
    {
        use std::path::{Component, Prefix};
        // dunce 在去前缀可能改变含义时会保留原路径；不能继续传给旧 Java。
        if matches!(root.components().next(), Some(Component::Prefix(prefix)) if matches!(
            prefix.kind(), Prefix::Verbatim(_) | Prefix::VerbatimUNC(_, _) | Prefix::VerbatimDisk(_) | Prefix::DeviceNS(_)
        )) {
            return Err(invalid());
        }
    }
    root.to_str()
        .filter(|path| {
            root.is_absolute()
                && !path.chars().any(|ch| ch == '"' || ch.is_control())
                && (cfg!(windows) || !path.contains('\\'))
        })
        .ok_or_else(invalid)
}

#[cfg(windows)]
mod windows;

fn invalid() -> CheckError {
    CheckError::new(
        "INVALID_REQUEST",
        "Java 任务目录无法安全传递；未启动工具，不会自动重试。",
    )
}

#[cfg(test)]
mod tests;

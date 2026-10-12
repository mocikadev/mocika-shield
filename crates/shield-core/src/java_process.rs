//! Java 工具的文本协议，不改变系统环境或文件系统路径编码。

use crate::utils::{no_window_command, strip_unc_prefix};
use std::ffi::OsStr;
use std::path::Path;
use std::process::Command;

const TEXT_PROPERTIES: &[&str] = &[
    "-Dfile.encoding=UTF-8",
    "-Dsun.stdout.encoding=UTF-8",
    "-Dsun.stderr.encoding=UTF-8",
    "-Dstdout.encoding=UTF-8",
    "-Dstderr.encoding=UTF-8",
    "-Duser.language=en",
    "-Duser.country=US",
];

/// Java 8 至新版本统一使用 UTF-8 工具输出，保留 Windows 无控制台行为。
/// 调用方在此后追加 -jar、工具参数；不得把这些 JVM 属性放到 -jar 后面。
pub fn java_command(path: impl AsRef<OsStr>) -> Command {
    let mut command = no_window_command(strip_unc_prefix(Path::new(path.as_ref())));
    command.args(TEXT_PROPERTIES);
    command
}

pub(crate) fn keytool_command(path: impl AsRef<OsStr>) -> Command {
    let mut command = no_window_command(strip_unc_prefix(Path::new(path.as_ref())));
    // keytool 的 JVM 参数需带 -J，不能直接复用 java 的命令行形式。
    command.args(
        TEXT_PROPERTIES
            .iter()
            .map(|property| format!("-J{property}")),
    );
    command
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn java参数先于工具参数且中文参数不拆分() {
        let mut command = java_command("java");
        command.args(["-jar", "中文工具 空格.jar", "中文应用 空格&符号.apk"]);
        let args: Vec<_> = command
            .get_args()
            .map(|arg| arg.to_str().unwrap())
            .collect();
        assert_eq!(&args[..TEXT_PROPERTIES.len()], TEXT_PROPERTIES);
        assert_eq!(
            &args[TEXT_PROPERTIES.len()..],
            ["-jar", "中文工具 空格.jar", "中文应用 空格&符号.apk"]
        );
    }

    #[test]
    fn keytool编码规则一致并使用专属前缀() {
        let command = keytool_command("keytool");
        let args: Vec<_> = command
            .get_args()
            .map(|arg| arg.to_str().unwrap())
            .collect();
        let expected: Vec<_> = TEXT_PROPERTIES
            .iter()
            .map(|arg| format!("-J{arg}"))
            .collect();
        assert_eq!(args, expected);
    }
}

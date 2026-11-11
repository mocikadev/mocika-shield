//! 固定工具入口与有界双流捕获；不选全局工具、不发布产物、不承担整组停止。
use super::*;
use crate::diagnostic::ToolName;
use std::{fs, process::ExitStatus, thread};

const LOG_LIMIT: usize = 64 * 1024;

#[derive(Clone, Copy)]
pub(super) enum JarTool {
    Apksigner,
    Apktool,
}

impl JarTool {
    fn name(self) -> ToolName {
        match self {
            Self::Apksigner => ToolName::Apksigner,
            Self::Apktool => ToolName::Apktool,
        }
    }
}

// 原工具可能打印业务路径或其他敏感文本；保持私有且不派生 Debug，不直接进入错误回执。
pub(crate) struct Captured {
    pub bytes: Vec<u8>,
    pub truncated: bool,
}

pub(crate) struct JarOutput {
    pub status: ExitStatus,
    pub stdout: Captured,
    pub stderr: Captured,
}

fn failed(tool: JarTool, code: Option<i32>) -> anyhow::Error {
    let mut diagnostic = Diagnostic::new(FailureCode::ToolProcessFailed);
    diagnostic.tool = Some(tool.name());
    diagnostic.exit_code = code;
    diagnostic.attach(anyhow::anyhow!(
        "固定 Java 工具调用失败或结果无效；未回退有损参数重试。"
    ))
}

fn capture(mut source: impl Read) -> std::io::Result<Captured> {
    let mut bytes = Vec::new();
    let mut truncated = false;
    let mut buffer = [0; 8192];
    loop {
        let count = match source.read(&mut buffer) {
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            result => result?,
        };
        if count == 0 {
            return Ok(Captured { bytes, truncated });
        }
        let keep = count.min(LOG_LIMIT - bytes.len());
        bytes.extend_from_slice(&buffer[..keep]);
        truncated |= keep != count;
        // 超量继续排空，不能让工具在 stdout／stderr 写满后卡住；总期限由外层监督。
    }
}

impl JavaToolContext {
    // 固定签名／验签语义的核心内部边界，不向入口提供通用 Command。
    pub(crate) fn run_apksigner(
        &self,
        java: &Path,
        jar: &Path,
        args: &[&str],
        passwords: Option<(&str, &str)>,
    ) -> Result<JarOutput> {
        if passwords.is_some_and(|(store, key)| store.contains('\0') || key.contains('\0')) {
            return Err(failed(JarTool::Apksigner, None));
        }
        let mut command = self.prepare_jar(java, JarTool::Apksigner, jar, args)?;
        if let Some((store, key)) = passwords {
            command
                .env(crate::java_process::KS_PASS_ENV, store)
                .env(crate::java_process::KEY_PASS_ENV, key);
        }
        Self::collect_jar(command, JarTool::Apksigner)
    }

    /// 仅查询明确选定的 apksigner 版本；不自动选择资源，也不执行签名。
    pub fn query_apksigner_version(&self, java: &Path, jar: &Path) -> Result<String> {
        self.jar_version(java, JarTool::Apksigner, jar, "version")
    }

    /// 仅查询明确选定的 apktool 版本；不自动选择资源，也不执行解包／重建。
    pub fn query_apktool_version(&self, java: &Path, jar: &Path) -> Result<String> {
        self.jar_version(java, JarTool::Apktool, jar, "--version")
    }

    fn jar_version(
        &self,
        java: &Path,
        tool: JarTool,
        jar: &Path,
        argument: &str,
    ) -> Result<String> {
        let command = self.prepare_jar(java, tool, jar, &[argument])?;
        let output = Self::collect_jar(command, tool)?;
        let value = std::str::from_utf8(&output.stdout.bytes)
            .ok()
            .map(str::trim)
            .filter(|value| {
                !value.is_empty()
                    && value.len() <= 128
                    && value.as_bytes()[0].is_ascii_digit()
                    && value
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || b".-+_".contains(&byte))
            });
        if !output.status.success() || output.stdout.truncated || output.stderr.truncated {
            return Err(failed(tool, output.status.code()));
        }
        value
            .map(str::to_owned)
            .ok_or_else(|| failed(tool, output.status.code()))
    }

    // 仅供核心固定用例编排；不对外提供任意参数／类路径的 Command 接口。
    pub(super) fn prepare_jar(
        &self,
        java: &Path,
        tool: JarTool,
        jar: &Path,
        args: &[&str],
    ) -> Result<Command> {
        if !jar.is_absolute()
            || !fs::symlink_metadata(jar).is_ok_and(|metadata| metadata.is_file())
            || args.is_empty()
            || args.iter().any(|arg| arg.contains('\0'))
        {
            return Err(failed(tool, None));
        }
        let jar = crate::utils::strip_unc_prefix(jar);
        let mut values = vec![tool.name().as_str(), jar.to_str().ok_or_else(invalid)?];
        values.extend_from_slice(args);
        let mut command = self.prepare_request(java, &values)?;
        command
            .env_remove(crate::java_process::KS_PASS_ENV)
            .env_remove(crate::java_process::KEY_PASS_ENV);
        Ok(command)
    }

    pub(super) fn collect_jar(mut command: Command, tool: JarTool) -> Result<JarOutput> {
        let mut child = command
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|_| failed(tool, None))?;
        let stdout = child.stdout.take().ok_or_else(|| failed(tool, None))?;
        let stderr = child.stderr.take().ok_or_else(|| failed(tool, None))?;
        // 两个有限读取线程仅排空输出，不向 stdin 写入、不持有租约；不承诺同步硬超时。
        // 后代若保留输出句柄，仍须由外层整组监督结束，不能提前清理工作根。
        thread::scope(|scope| {
            let stdout = scope.spawn(|| capture(stdout));
            let stderr = scope.spawn(|| capture(stderr));
            let status = child.wait();
            let stdout = stdout.join();
            let stderr = stderr.join();
            let status = status.map_err(|_| failed(tool, None))?;
            let stdout = stdout
                .map_err(|_| failed(tool, status.code()))?
                .map_err(|_| failed(tool, status.code()))?;
            let stderr = stderr
                .map_err(|_| failed(tool, status.code()))?
                .map_err(|_| failed(tool, status.code()))?;
            Ok(JarOutput {
                status,
                stdout,
                stderr,
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 日志保留边界并继续排空且截断状态明确() {
        for count in [0, 1, LOG_LIMIT, LOG_LIMIT + 1, 3 * LOG_LIMIT] {
            let mut input = std::io::Cursor::new(vec![b'a'; count]);
            let result = capture(&mut input).unwrap();
            assert_eq!(input.position(), count as u64);
            assert_eq!(result.bytes.len(), count.min(LOG_LIMIT));
            assert_eq!(result.truncated, count > LOG_LIMIT);
        }
    }
}

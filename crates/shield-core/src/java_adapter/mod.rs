//! 固定辅助资源与借用工作目录；不拥有任务、最终产物或证书资料库。
use crate::{
    diagnostic::{Diagnostic, FailureCode},
    utils::java_command,
};
use anyhow::Result;
use serde::Deserialize;
use std::{
    io::{Read, Write},
    path::Path,
    process::{Command, Stdio},
};
mod apktool;
mod certificate_session;
mod certificates;
pub use apktool::ApktoolProject;
pub(crate) use certificate_session::certificate_session;
mod creation;
mod jars;
pub(crate) use jars::{Captured, JarOutput};
#[cfg(any(windows, test))]
mod local_session;
mod location;
#[cfg(any(windows, test))]
pub(crate) use local_session::LocalSession;
mod private_io;
mod request;
mod resource;
mod resource_location;
pub use resource_location::find_java_adapter;
#[cfg(test)]
mod tests;

const PROTOCOL: u32 = 4;

fn invalid() -> anyhow::Error {
    Diagnostic::new(FailureCode::ToolProcessFailed).attach(anyhow::anyhow!(
        "Java 适配资源、请求或工作目录无效；请重新构建匹配资源，未回退有损参数。"
    ))
}

fn handshake_failed() -> anyhow::Error {
    Diagnostic::new(FailureCode::ToolProcessFailed).attach(anyhow::anyhow!(
        "Java 适配兼容握手失败；未执行业务工具，请核对所选 Java 与引导目录。"
    ))
}

/// 显式选定资源和本次私有工作根；不搜索全局路径，不负责删除借用目录。
/// 提供兼容握手、显式证书与 APK 工具语义；普通入口的选择与所有权由外层封装负责。
pub struct JavaToolContext {
    resource: resource::Resource,
    workspace: location::Workspace,
}

impl JavaToolContext {
    // 创建的材料仍归外层根所有；失败也不擅自清理，避免删除尚存后代的工作目录。
    pub(crate) fn material_directory(&self) -> Result<std::path::PathBuf> {
        self.workspace.verify()?;
        self.resource.snapshot()?;
        location::bootstrap_directory(self.workspace.path())?;
        let path = private_io::directory(self.workspace.path())?;
        self.workspace.verify()?;
        Ok(path)
    }

    pub fn new(adapter: &Path, workspace: &Path) -> Result<Self> {
        if !adapter.is_absolute() || !workspace.is_absolute() {
            return Err(invalid());
        }
        Ok(Self {
            resource: resource::Resource::open(adapter)?,
            workspace: location::Workspace::new(workspace)?,
        })
    }

    /// 返回本次选定资源的实际摘要；不作为发布签名或替代调用方的文件身份检查。
    pub fn resource_sha256(&self) -> [u8; 32] {
        self.resource.digest
    }

    /// 执行无业务副作用的兼容握手。取消与总期限仍由外层执行器负责。
    /// 本方法不承诺硬超时；临时材料留在调用方工作根，供其确认停止后统一清理。
    pub fn verify_runtime(&self, java: &Path) -> Result<()> {
        let (status, output) = self.invoke(java, &["hello"], None, 4096)?;
        if !status.success() {
            return Err(handshake_failed());
        }
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Reply {
            protocol_version: u32,
            application_version: String,
            operation: String,
        }
        let reply: Reply = serde_json::from_slice(&output).map_err(|_| handshake_failed())?;
        if reply.protocol_version != PROTOCOL
            || reply.application_version != env!("CARGO_PKG_VERSION")
            || reply.operation != "hello"
        {
            return Err(handshake_failed());
        }
        Ok(())
    }

    fn invoke(
        &self,
        java: &Path,
        values: &[&str],
        password: Option<&str>,
        limit: u64,
    ) -> Result<(std::process::ExitStatus, Vec<u8>)> {
        if password.is_some_and(|password| password.contains('\0')) {
            return Err(invalid());
        }
        let mut command = self.prepare_request(java, values)?;
        command
            .env_remove(crate::java_process::KS_PASS_ENV)
            .env_remove(crate::java_process::KEY_PASS_ENV);
        if let Some(password) = password {
            command.env(crate::java_process::KS_PASS_ENV, password);
        }
        Self::collect(command, limit)
    }

    fn collect(mut command: Command, limit: u64) -> Result<(std::process::ExitStatus, Vec<u8>)> {
        let mut child = command
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()?;
        let mut output = Vec::new();
        let read = child
            .stdout
            .take()
            .ok_or_else(invalid)?
            .take(limit + 1)
            .read_to_end(&mut output);
        if read.is_err() || output.len() as u64 > limit {
            let _ = child.kill();
            let _ = child.wait();
            // 只停止当前助手，不报告整个任务／后代已停止，也不删目录。
            return Err(
                Diagnostic::new(FailureCode::ToolProcessFailed).attach(anyhow::anyhow!(
                    "Java 适配结果读取失败或超出预算；未回退旧参数重试。"
                )),
            );
        }
        Ok((child.wait()?, output))
    }

    #[cfg(test)]
    fn prepare(&self, java: &Path) -> Result<Command> {
        self.prepare_request(java, &["hello"])
    }

    fn prepare_request(&self, java: &Path, values: &[&str]) -> Result<Command> {
        if !java.is_absolute() || !java.is_file() {
            return Err(invalid());
        }
        self.workspace.verify()?;
        let data = self.resource.snapshot()?;
        location::bootstrap_directory(self.workspace.path())?;
        let directory = private_io::directory(self.workspace.path())?;
        let bootstrap = location::bootstrap_directory(&directory)?;
        let jar_path = directory.join("java-adapter.jar");
        let mut jar = private_io::create_file(&jar_path)?;
        jar.write_all(&data)?;
        let jar_identity = private_io::identity(&jar)?;
        drop(jar);
        let copied = private_io::read_back(&jar_path, jar_identity, data.len())?;
        let input = request::input(&directory, &request::encode(values)?)?;
        private_io::validate_directory(&directory)?;
        private_io::validate_file(&copied)?;
        private_io::validate_file(&input)?;
        let mut command = java_command(java);
        // apktool 提取的 Native 工具及其临时文件只落本次引导目录，不使用全局临时缓存。
        // 固定相对属性保持原生 argv 为 ASCII；业务及框架路径仍由 UTF-8 请求传递。
        if values.first() == Some(&"apktool") {
            command.arg("-Djava.io.tmpdir=.");
        }
        command
            .current_dir(bootstrap)
            .args(["-jar", "java-adapter.jar"])
            .stdin(input);
        // 启动后清理权在外层任务；不能依靠该上下文 Drop 宣称 Native 后代已停止。
        self.workspace.verify()?;
        // 本次目录自创建起即交由调用方管理，准备失败也不擅自递归清理。
        Ok(command)
    }
}

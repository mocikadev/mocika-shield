//! 显式解包项目与重建候选；仅借用工作根，不拥有最终提交或整组停止。
use super::{jars::JarTool, *};
use crate::diagnostic::ToolName;
use std::{fs, path::PathBuf};

/// 本次解包项目，框架缓存与候选均留在调用方的私有工作根。
/// 不自动清理，不选择全局工具；修改目录后可重建多份互不覆盖的候选。
pub struct ApktoolProject<'a> {
    context: &'a JavaToolContext,
    java: PathBuf,
    jar: PathBuf,
    directory: PathBuf,
    framework: PathBuf,
}

impl JavaToolContext {
    /// 使用显式 Java／apktool 解包并保留 DEX 原字节；不修改输入、不注入壳。
    /// 返回项目的目录供核心业务修改，失败材料仍由调用方确认停止后回收。
    pub fn decode_apk(&self, java: &Path, jar: &Path, input: &Path) -> Result<ApktoolProject<'_>> {
        let java = absolute(java)?;
        let jar = absolute(jar)?;
        let input = absolute(input)?;
        if !fs::symlink_metadata(&input)?.is_file() {
            return Err(invalid());
        }
        let root = self.material_directory()?;
        let framework = private_io::directory(&root)?;
        let project = ApktoolProject {
            context: self,
            java,
            jar,
            directory: root.join("decoded"),
            framework,
        };
        project.run(
            &[
                "d",
                text(&input)?,
                "-o",
                text(&project.directory)?,
                "--no-src",
                "-p",
                text(&project.framework)?,
            ],
            FailureCode::ToolProcessFailed,
        )?;
        if !project.directory.join("AndroidManifest.xml").is_file()
            || !project.directory.join("apktool.yml").is_file()
        {
            return Err(failure(FailureCode::ToolProcessFailed));
        }
        Ok(project)
    }
}

impl ApktoolProject<'_> {
    pub fn directory(&self) -> &Path {
        &self.directory
    }

    /// 重建到新建私有目录中的候选；不覆盖输入或已有输出，不负责对齐和签名。
    /// 同步调用没有硬超时；Native 后代停止与材料回收由外层执行器负责。
    pub fn rebuild(&self) -> Result<PathBuf> {
        // Java 能读取 Unicode 请求，不代表它启动的 Native 工具能读取相同路径。
        // Windows 仅向 Native 链路提供同目录已有 ASCII 名称；对外仍返回原始路径。
        let directory = native_directory(&self.directory)?;
        let framework = native_directory(&self.framework)?;
        let output_root = self.context.material_directory()?;
        let native_output = native_directory(&output_root)?.join("candidate.apk");
        let output = output_root.join("candidate.apk");
        self.run(
            &[
                "b",
                text(&directory)?,
                "-o",
                text(&native_output)?,
                "-f",
                "-p",
                text(&framework)?,
            ],
            FailureCode::ApkRepackFailed,
        )?;
        if !binary_apk(&output).unwrap_or(false) {
            return Err(failure(FailureCode::ApkRepackFailed));
        }
        Ok(output)
    }

    fn run(&self, args: &[&str], fallback: FailureCode) -> Result<()> {
        let command = self
            .context
            .prepare_jar(&self.java, JarTool::Apktool, &self.jar, args)?;
        let output = JavaToolContext::collect_jar(command, JarTool::Apktool)?;
        if !output.status.success() || output.stdout.truncated || output.stderr.truncated {
            #[cfg(all(test, windows))]
            {
                // 仅固定失败特征，不能把原工具路径、日志或环境写入专项回执。
                let text = String::from_utf8_lossy(&output.stderr.bytes).to_ascii_lowercase();
                println!(
                    "apktool 固定失败诊断：{}",
                    serde_json::json!({
                        "stage": args[0], "exit": output.status.code(),
                        "native_execution": text.contains("could not exec"),
                        "native_extraction": text.contains("could not extract"),
                        "empty_package": text.contains("all packages being linked must have a name"),
                        "missing_file": text.contains("no such file") || text.contains("cannot find the file"),
                        "permission": text.contains("permission denied") || text.contains("access is denied"),
                        "adapter_exception": text.contains("java_adapter_tool_exception")
                    })
                );
            }
            let mut error = Diagnostic::from_tool_streams(
                ToolName::Apktool,
                output.status.code(),
                &output.stdout.bytes,
                &output.stderr.bytes,
            );
            if error.code == FailureCode::ToolProcessFailed {
                error.code = fallback;
            }
            // 仅用于有限分类，不回显 apktool／Native 原始路径、日志或异常。
            return Err(error.attach(anyhow::anyhow!(
                "APK 解包或重建失败；诊断已截断：{}；未发布候选或回退旧参数。",
                output.stdout.truncated || output.stderr.truncated
            )));
        }
        Ok(())
    }
}

fn absolute(path: &Path) -> Result<PathBuf> {
    Ok(crate::utils::strip_unc_prefix(&std::path::absolute(path)?))
}

fn native_directory(path: &Path) -> Result<PathBuf> {
    #[cfg(windows)]
    {
        crate::java_directory::ascii_directory(path).map_err(|_| {
            let mut diagnostic = Diagnostic::new(FailureCode::ToolProcessFailed);
            diagnostic.tool = Some(ToolName::Apktool);
            diagnostic.attach(anyhow::anyhow!(
                "Native 重建目录没有可核对身份的安全 ASCII 名称；未启动重建、改名、移动或自动重试。"
            ))
        })
    }
    #[cfg(not(windows))]
    {
        Ok(path.to_owned())
    }
}

// 只做 ZIP／二进制清单头的基本检查，不冒充完整签名或安装验证。
fn binary_apk(path: &Path) -> Result<bool> {
    if !fs::symlink_metadata(path)?.is_file() {
        return Ok(false);
    }
    let mut archive = zip::ZipArchive::new(fs::File::open(path)?)?;
    let mut header = [0; 4];
    archive
        .by_name("AndroidManifest.xml")?
        .read_exact(&mut header)?;
    Ok(header == [3, 0, 8, 0])
}

fn text(path: &Path) -> Result<&str> {
    path.to_str().ok_or_else(invalid)
}

fn failure(code: FailureCode) -> anyhow::Error {
    let mut error = Diagnostic::new(code);
    error.tool = Some(ToolName::Apktool);
    error.exit_code = Some(0);
    error.attach(anyhow::anyhow!(
        "APK 工具已返回成功但产物无效；未发布候选。"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn 成功状态不能代替候选基本结构检查() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("candidate.apk");
        assert!(binary_apk(&path).is_err());
        fs::write(&path, b"invalid-zip").unwrap();
        assert!(binary_apk(&path).is_err());
        for header in [b"<man".as_slice(), &[3, 0, 8, 0]] {
            let mut archive = zip::ZipWriter::new(fs::File::create(&path).unwrap());
            archive
                .start_file(
                    "AndroidManifest.xml",
                    zip::write::SimpleFileOptions::default(),
                )
                .unwrap();
            archive.write_all(header).unwrap();
            archive.finish().unwrap();
            assert_eq!(binary_apk(&path).unwrap(), header == [3, 0, 8, 0]);
        }
        let diagnostic = Diagnostic::from_error(&failure(FailureCode::ApkRepackFailed));
        assert_eq!(diagnostic.exit_code, Some(0));
        assert_eq!(diagnostic.tool, Some(ToolName::Apktool));
    }
}

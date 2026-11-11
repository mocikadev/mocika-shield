//! 加固工具的显式／旧调用和项目生命周期；不承载壳业务、任务监督或最终无覆盖提交。
use super::ProtectOptions;
use crate::{
    apk_inspect::{check_apk, extract_apk_cert_fingerprint, ApkCheckOutcome},
    diagnostic::{Diagnostic, FailureCode, ToolName},
    utils::{create_temp_dir, find_apksigner, find_apktool, find_java, java_command},
    ApktoolProject, JavaToolContext,
};
use anyhow::Result;
use std::{
    path::{Path, PathBuf},
    process::Stdio,
};
use tempfile::TempDir;

pub(super) struct Tools<'a> {
    java: PathBuf,
    apktool: PathBuf,
    signer: PathBuf,
    context: Option<&'a JavaToolContext>,
}

pub(super) enum Project<'a> {
    Local { _owner: TempDir, directory: PathBuf },
    Borrowed(ApktoolProject<'a>),
}

impl<'a> Tools<'a> {
    pub fn new(
        opts: &ProtectOptions,
        adapter: Option<(&'a JavaToolContext, &Path)>,
    ) -> Result<Self> {
        Ok(Self {
            java: match adapter {
                Some((_, java)) if java.is_absolute() && java.is_file() => {
                    crate::utils::strip_unc_prefix(java)
                }
                Some(_) => return Err(invalid()),
                None => find_java()?,
            },
            apktool: selected(&opts.apktool_path, find_apktool, "apktool.jar")?,
            signer: selected(&opts.apksigner_path, find_apksigner, "apksigner.jar")?,
            context: adapter.map(|(context, _)| context),
        })
    }

    pub fn inspect(&self, input: &Path) -> Result<ApkCheckOutcome> {
        match self.context {
            Some(context) => {
                crate::apk_inspect::check_apk_with_context(context, &self.java, input, &self.signer)
            }
            None => check_apk(input, Some(&self.signer)),
        }
    }

    pub fn fingerprint(&self, input: &Path) -> Result<String> {
        match self.context {
            Some(context) => crate::extract_apk_cert_fingerprint_with_context(
                context,
                &self.java,
                input,
                &self.signer,
            ),
            None => extract_apk_cert_fingerprint(input, Some(&self.signer)),
        }
    }

    pub fn decode(&self, input: &Path) -> Result<Project<'a>> {
        if let Some(context) = self.context {
            return context
                .decode_apk(&self.java, &self.apktool, input)
                .map(Project::Borrowed);
        }
        let owner = create_temp_dir("shield-")?;
        let directory = owner.path().join("apk");
        run_apktool_command(
            &self.java,
            &[
                "-jar",
                text(&self.apktool)?,
                "d",
                text(input)?,
                "-o",
                text(&directory)?,
                "-f",
                "--no-src",
            ],
            FailureCode::ToolProcessFailed,
        )?;
        Ok(Project::Local {
            _owner: owner,
            directory,
        })
    }

    pub fn rebuild(&self, project: &Project<'_>, output: &Path) -> Result<PathBuf> {
        match project {
            Project::Borrowed(project) => project.rebuild(),
            Project::Local { directory, .. } => {
                run_apktool_command(
                    &self.java,
                    &[
                        "-jar",
                        text(&self.apktool)?,
                        "b",
                        text(directory)?,
                        "-o",
                        text(output)?,
                        "-f",
                    ],
                    FailureCode::ApkRepackFailed,
                )?;
                Ok(output.to_owned())
            }
        }
    }
}

impl Project<'_> {
    pub fn directory(&self) -> &Path {
        match self {
            Self::Local { directory, .. } => directory,
            Self::Borrowed(project) => project.directory(),
        }
    }
}

fn text(path: &Path) -> Result<&str> {
    path.to_str().ok_or_else(invalid)
}

fn invalid() -> anyhow::Error {
    Diagnostic::new(FailureCode::ToolNotFound).attach(anyhow::anyhow!(
        "显式加固需要已选定的 Java、apktool、apksigner 和壳资源；未搜索全局材料。"
    ))
}

fn selected(
    path: &Option<PathBuf>,
    fallback: impl FnOnce() -> Result<PathBuf>,
    name: &str,
) -> Result<PathBuf> {
    match path {
        Some(path) if path.exists() => Ok(path.clone()),
        Some(path) => Err(anyhow::anyhow!(
            "配置的 {name} 路径不存在: {}",
            path.display()
        )),
        None => fallback(),
    }
}

pub(super) fn explicit_options(opts: &ProtectOptions) -> Result<ProtectOptions> {
    normalize_options(opts, true)
}

#[cfg(any(windows, test))]
pub(super) fn ordinary_options(opts: &ProtectOptions) -> Result<ProtectOptions> {
    let mut selected = opts.clone();
    if selected.apktool_path.is_none() {
        selected.apktool_path = Some(
            find_apktool()
                .map_err(|error| Diagnostic::new(FailureCode::ToolNotFound).attach(error))?,
        );
    }
    if selected.apksigner_path.is_none() {
        selected.apksigner_path = Some(
            find_apksigner()
                .map_err(|error| Diagnostic::new(FailureCode::ToolNotFound).attach(error))?,
        );
    }
    normalize_options(&selected, false)
}

fn normalize_options(opts: &ProtectOptions, require_resources: bool) -> Result<ProtectOptions> {
    let mut result = opts.clone();
    let absolute = |path: &Path| -> Result<PathBuf> {
        Ok(crate::utils::strip_unc_prefix(&std::path::absolute(path)?))
    };
    result.input = absolute(&result.input)?;
    result.output = absolute(&result.output)?;
    for (material, required) in [
        (&mut result.apktool_path, true),
        (&mut result.apksigner_path, true),
        (&mut result.resources_path, require_resources),
    ] {
        let Some(path) = material.as_ref() else {
            if !required {
                continue;
            }
            return Err(invalid());
        };
        if !path.is_file() {
            return Err(invalid());
        }
        *material = Some(absolute(path)?);
    }
    if result.input == result.output
        || std::fs::canonicalize(&result.input)
            .ok()
            .is_some_and(|input| {
                std::fs::canonicalize(&result.output).is_ok_and(|output| input == output)
            })
    {
        anyhow::bail!("显式加固不能将输入 APK 自身作为输出，未启动工具或覆盖原包");
    }
    Ok(result)
}

fn run_apktool_command(java: &Path, args: &[&str], fallback: FailureCode) -> Result<()> {
    let output = java_command(java)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .map_err(|error| {
            Diagnostic::from_io(&error).attach(anyhow::anyhow!("执行命令失败: {:?}: {error}", java))
        })?;
    if !output.status.success() {
        return Err(apktool_command_error(
            java,
            fallback,
            output.status.code(),
            &output.stdout,
            &output.stderr,
        ));
    }
    Ok(())
}

pub(super) fn apktool_command_error(
    java: &Path,
    fallback: FailureCode,
    exit_code: Option<i32>,
    stdout: &[u8],
    stderr: &[u8],
) -> anyhow::Error {
    let mut diagnostic =
        Diagnostic::from_tool_streams(ToolName::Apktool, exit_code, stdout, stderr);
    if diagnostic.code == FailureCode::ToolProcessFailed {
        diagnostic.code = fallback;
    }
    diagnostic.attach(anyhow::anyhow!(
        "命令执行失败: {:?}\n错误: {}",
        java,
        String::from_utf8_lossy(if stderr.is_empty() { stdout } else { stderr })
    ))
}

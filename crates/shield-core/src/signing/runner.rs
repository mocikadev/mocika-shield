//! 共用签名流程；工具选择与传输不同，但对齐、进度及输出语义只有一份。
use super::*;
use crate::{
    diagnostic::{Diagnostic, FailureCode, ToolName},
    java_adapter::{Captured, JarOutput},
    utils::{find_java, java_command, strip_unc_prefix},
    zipalign::align_apk,
    JavaToolContext,
};
use tempfile::TempDir;

enum Workspace {
    Local(TempDir),
    Borrowed(PathBuf),
}

impl Workspace {
    fn path(&self) -> &Path {
        match self {
            Self::Local(directory) => directory.path(),
            Self::Borrowed(path) => path,
        }
    }
}

fn path(path: &Path, absolute: bool) -> Result<PathBuf> {
    if absolute {
        Ok(strip_unc_prefix(&std::path::absolute(path)?))
    } else {
        Ok(path.to_owned())
    }
}

fn invalid() -> anyhow::Error {
    let mut diagnostic = Diagnostic::new(FailureCode::ToolProcessFailed);
    diagnostic.tool = Some(ToolName::Apksigner);
    diagnostic.attach(anyhow::anyhow!(
        "显式签名需要已选定的 Java、apksigner 及有效参数；未搜索全局工具或回退旧参数。"
    ))
}

fn check_inplace_volume(workspace: &Path, input: &Path) -> Result<()> {
    #[cfg(unix)]
    let same = {
        use std::os::unix::fs::MetadataExt;
        std::fs::metadata(workspace)?.dev() == std::fs::metadata(input)?.dev()
    };
    #[cfg(windows)]
    let same = {
        let directory = crate::java_directory::open_directory(workspace)?;
        let input = std::fs::File::open(input)?;
        crate::java_directory::identity(&directory)?.0 == crate::java_directory::identity(&input)?.0
    };
    if !same {
        return Err(Diagnostic::new(FailureCode::ToolProcessFailed).attach(anyhow::anyhow!(
            "显式原地签名的工作根与输入 APK 不在同一文件系统；请由调用方选定同卷私有工作根，未删除或替换原包。"
        )));
    }
    Ok(())
}

pub(super) fn run(
    opts: &SignOptions,
    adapter: Option<(&JavaToolContext, &Path)>,
    mut on_progress: impl FnMut(SigningProgressStep) -> std::result::Result<(), String>,
) -> Result<()> {
    on_progress(SigningProgressStep::Prepare).map_err(anyhow::Error::msg)?;
    run_prepared(opts, adapter, on_progress)
}

#[cfg(any(windows, test))]
pub(crate) fn run_default(
    opts: &SignOptions,
    mut on_progress: impl FnMut(SigningProgressStep) -> std::result::Result<(), String>,
) -> Result<()> {
    // 保留原有首次取消检查，不能在准备回调拒绝后再握手或创建材料。
    on_progress(SigningProgressStep::Prepare).map_err(anyhow::Error::msg)?;
    let mut selected = opts.clone();
    if selected.apksigner_path.is_none() {
        selected.apksigner_path = Some(
            super::find_apksigner()
                .map_err(|error| Diagnostic::new(FailureCode::ToolNotFound).attach(error))?,
        );
    }
    let output = opts.output_path.as_deref().unwrap_or(&opts.apk_path);
    let session = crate::java_adapter::LocalSession::for_output(output)?;
    let result = run_prepared(
        &selected,
        Some((&session.context, &session.java)),
        on_progress,
    );
    session.finish(result)
}

fn run_prepared(
    opts: &SignOptions,
    adapter: Option<(&JavaToolContext, &Path)>,
    mut on_progress: impl FnMut(SigningProgressStep) -> std::result::Result<(), String>,
) -> Result<()> {
    anyhow::ensure!(
        !opts.signing_versions.v4 || opts.signing_versions.v2 || opts.signing_versions.v3,
        "V4 签名需要同时启用 V2 或 V3"
    );
    let adapted = adapter.is_some();
    let apksigner = match &opts.apksigner_path {
        Some(p) if p.exists() => path(p, adapted)?,
        Some(p) => anyhow::bail!("配置的 apksigner.jar 路径不存在: {}", p.display()),
        None if adapted => return Err(invalid()),
        None => super::find_apksigner()
            .map_err(|error| Diagnostic::new(FailureCode::ToolNotFound).attach(error))?,
    };
    let java = match adapter {
        Some((_, java)) if java.is_absolute() && java.is_file() => strip_unc_prefix(java),
        Some(_) => return Err(invalid()),
        None => {
            find_java().map_err(|error| Diagnostic::new(FailureCode::ToolNotFound).attach(error))?
        }
    };
    if adapted
        && [&opts.key_alias, &opts.keystore_password, &opts.key_password]
            .iter()
            .any(|value| value.contains('\0'))
    {
        return Err(invalid());
    }
    let input = path(&opts.apk_path, adapted)?;
    let keystore = path(&opts.keystore_path, adapted)?;
    keystore.to_str().context("keystore 路径包含非法字符")?;
    apksigner.to_str().context("apksigner 路径包含非法字符")?;
    let final_output = path(
        opts.output_path.as_deref().unwrap_or(&opts.apk_path),
        adapted,
    )?;
    let workspace = match adapter {
        Some((context, _)) => Workspace::Borrowed(context.material_directory()?),
        None => Workspace::Local(TempDir::new().context("创建签名临时目录失败")?),
    };
    if adapted && final_output == input {
        // 借用根不能自动搬到其他卷；避免先删除原包，再因跨卷 rename 失败造成丢失。
        check_inplace_volume(workspace.path(), &input)?;
    }
    let aligned = workspace.path().join("aligned.apk");
    on_progress(SigningProgressStep::Align).map_err(anyhow::Error::msg)?;
    std::fs::copy(&input, &aligned)
        .with_context(|| format!("复制待签名 APK 到临时目录失败: {}", input.display()))?;
    align_apk(&aligned)
        .context("内置 APK 对齐失败")
        .map_err(|error| {
            Diagnostic::from_error(&error)
                .or_code(FailureCode::AlignmentFailed)
                .attach(error)
        })?;
    let artifacts = super::artifacts::Artifacts::new(&final_output, opts.signing_versions.v4)?;
    let sign_output = artifacts.staged_apk();
    let args = arguments(opts, &keystore, &aligned, &sign_output)?;
    let args: Vec<_> = args.iter().map(String::as_str).collect();
    on_progress(SigningProgressStep::Sign).map_err(anyhow::Error::msg)?;
    let output = match adapter {
        Some((context, _)) => context.run_apksigner(
            &java,
            &apksigner,
            &args,
            Some((&opts.keystore_password, &opts.key_password)),
        )?,
        None => {
            let output = java_command(&java)
                .arg("-jar")
                .arg(&apksigner)
                .args(&args)
                .env(crate::java_process::KS_PASS_ENV, &opts.keystore_password)
                .env(crate::java_process::KEY_PASS_ENV, &opts.key_password)
                .output()
                .context("启动 apksigner 失败，请确认 Java 可用")?;
            JarOutput {
                status: output.status,
                stdout: Captured {
                    bytes: output.stdout,
                    truncated: false,
                },
                stderr: Captured {
                    bytes: output.stderr,
                    truncated: false,
                },
            }
        }
    };
    if !output.status.success() {
        return Err(if adapted {
            super::fixed_failure(&output, "APK 签名失败")
        } else {
            let code = output.status.code().unwrap_or(-1);
            Diagnostic::from_tool_streams(
                ToolName::Apksigner,
                output.status.code(),
                &output.stdout.bytes,
                &output.stderr.bytes,
            )
            .attach(anyhow::anyhow!(
                "签名失败（apksigner 退出码 {code}）：{}",
                super::classify_apksigner_error(
                    &String::from_utf8_lossy(&output.stderr.bytes),
                    &String::from_utf8_lossy(&output.stdout.bytes)
                )
            ))
        });
    }
    artifacts.validate()?;
    on_progress(SigningProgressStep::Commit).map_err(anyhow::Error::msg)?;
    artifacts.publish()?;
    Ok(())
}

fn arguments(
    opts: &SignOptions,
    keystore: &Path,
    input: &Path,
    output: &Path,
) -> Result<Vec<String>> {
    let ks_pass = format!("env:{}", crate::java_process::KS_PASS_ENV);
    let key_pass = format!("env:{}", crate::java_process::KEY_PASS_ENV);
    let path = |value: &Path| {
        value
            .to_str()
            .map(str::to_owned)
            .context("签名路径包含非法字符")
    };
    let mut args = vec![
        "sign".into(),
        "--ks-type".into(),
        opts.keystore_type.as_str().into(),
        "--ks".into(),
        path(keystore)?,
        "--ks-key-alias".into(),
        opts.key_alias.clone(),
        "--ks-pass".into(),
        ks_pass,
        "--key-pass".into(),
        key_pass,
    ];
    let versions = &opts.signing_versions;
    for (option, enabled) in [
        ("--v1-signing-enabled", versions.v1),
        ("--v2-signing-enabled", versions.v2),
        ("--v3-signing-enabled", versions.v3),
        ("--v4-signing-enabled", versions.v4),
    ] {
        args.extend([option.into(), super::bool_str(enabled).into()]);
    }
    args.extend(["--out".into(), path(output)?, path(input)?]);
    Ok(args)
}

#[cfg(test)]
mod tests;

use anyhow::Result;
use colored::Colorize;
use rand::RngCore;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

use crate::error::ShieldError;
use crate::protect::{
    dex::process_dex,
    manifest::modify_manifest,
    runtime::{inject_runtime, read_stub_application},
    signature::extract_apk_signature,
};
use crate::utils::is_json_mode;
use crate::utils::{
    create_temp_dir, find_apktool, find_java, find_runtime_resources, human_size, print_step,
    print_success, run_command,
};
use crate::zipalign::align_apk;

pub fn execute(input: &Path, output: &Path) -> Result<()> {
    let opts = ProtectOptions {
        input: input.to_path_buf(),
        output: output.to_path_buf(),
        apktool_path: None,
        resources_path: None,
    };

    protect_apk(&opts, |_| {}, Arc::new(AtomicBool::new(false)))?;
    Ok(())
}

#[derive(Debug, Clone)]
pub struct ProtectOptions {
    pub input: PathBuf,
    pub output: PathBuf,
    /// 用户自定义 apktool.jar 路径（优先于自动查找）
    pub apktool_path: Option<PathBuf>,
    /// 用户自定义 resources.zip 路径（优先于自动查找）
    pub resources_path: Option<PathBuf>,
}

#[derive(Debug, Clone)]
pub struct ProgressEvent {
    pub step: ProgressStep,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProgressStep {
    CheckTools,
    Unpack,
    ModifyManifest,
    ProcessDex,
    InjectRuntime,
    Repack,
    AlignApk,
}

pub fn protect_apk(
    opts: &ProtectOptions,
    on_progress: impl Fn(ProgressEvent) + Send + 'static,
    cancel: Arc<AtomicBool>,
) -> std::result::Result<(), ShieldError> {
    if !opts.input.exists() {
        return Err(ShieldError::FileNotFound(opts.input.display().to_string()));
    }

    let apktool = match &opts.apktool_path {
        Some(p) if p.exists() => p.clone(),
        Some(p) => {
            return Err(ShieldError::from(anyhow::anyhow!(
                "配置的 apktool.jar 路径不存在: {}",
                p.display()
            )))
        }
        None => find_apktool().map_err(ShieldError::from)?,
    };
    let runtime_resources = match &opts.resources_path {
        Some(p) if p.exists() => p.clone(),
        Some(p) => {
            return Err(ShieldError::from(anyhow::anyhow!(
                "配置的 resources.zip 路径不存在: {}",
                p.display()
            )))
        }
        None => find_runtime_resources().map_err(ShieldError::from)?,
    };

    if !is_json_mode() {
        println!("{}", "========================================".cyan());
        println!("{}", "Mocika Shield - APK Protection".cyan().bold());
        println!("{}", "========================================".cyan());
        println!("输入APK: {:?}", opts.input);
        println!("输出APK: {:?}", opts.output);
        println!("{}", "========================================".cyan());
    }

    let java = find_java().map_err(ShieldError::from)?;

    emit_progress(&on_progress, &cancel, ProgressStep::CheckTools, "检查工具")?;
    print_step("检查工具");
    find_apktool().map_err(ShieldError::from)?;
    find_runtime_resources().map_err(ShieldError::from)?;
    print_success("所有工具就绪");

    let temp_dir = create_temp_dir("shield-").map_err(ShieldError::from)?;
    let apk_dir = temp_dir.path().join("apk");

    emit_progress(&on_progress, &cancel, ProgressStep::Unpack, "解包APK")?;
    print_step("解包APK");
    run_command(
        &java,
        &[
            "-jar",
            apktool.to_str().unwrap(),
            "d",
            opts.input.to_str().unwrap(),
            "-o",
            apk_dir.to_str().unwrap(),
            "-f",
            "--no-src",
        ],
        None,
    )
    .map_err(ShieldError::from)?;
    print_success("解包完成");

    let stub_app = read_stub_application(&runtime_resources).map_err(ShieldError::from)?;

    emit_progress(
        &on_progress,
        &cancel,
        ProgressStep::ModifyManifest,
        "修改AndroidManifest.xml",
    )?;
    print_step("修改AndroidManifest.xml");
    modify_manifest(&apk_dir, &stub_app).map_err(ShieldError::from)?;
    print_success("Manifest修改完成");

    emit_progress(
        &on_progress,
        &cancel,
        ProgressStep::ProcessDex,
        "处理DEX文件",
    )?;
    print_internal_step("提取APK签名");
    let signature =
        extract_apk_signature(&opts.input, &apktool, &java).map_err(ShieldError::from)?;

    let mut ikm = [0u8; 32];
    rand::rng().fill_bytes(&mut ikm);

    print_step("处理DEX文件");
    process_dex(&apk_dir, &signature, &ikm).map_err(ShieldError::from)?;

    emit_progress(
        &on_progress,
        &cancel,
        ProgressStep::InjectRuntime,
        "注入Runtime库",
    )?;
    print_step("注入Runtime库");
    inject_runtime(&apk_dir, &runtime_resources, &opts.input).map_err(ShieldError::from)?;
    print_success("Runtime库注入完成");

    emit_progress(&on_progress, &cancel, ProgressStep::Repack, "重打包APK")?;
    print_step("重打包APK");
    run_command(
        &java,
        &[
            "-jar",
            apktool.to_str().unwrap(),
            "b",
            apk_dir.to_str().unwrap(),
            "-o",
            opts.output.to_str().unwrap(),
            "-f",
        ],
        None,
    )
    .map_err(ShieldError::from)?;

    let input_size = fs::metadata(&opts.input)
        .map_err(anyhow::Error::from)
        .map_err(ShieldError::from)?
        .len();
    let output_size = fs::metadata(&opts.output)
        .map_err(anyhow::Error::from)
        .map_err(ShieldError::from)?
        .len();
    let ratio = 100.0 * output_size as f64 / input_size as f64;
    print_success(&format!(
        "APK重打包完成: {} -> {} ({:.1}%)",
        human_size(input_size),
        human_size(output_size),
        ratio
    ));

    emit_progress(&on_progress, &cancel, ProgressStep::AlignApk, "对齐APK数据")?;
    print_step("对齐APK数据");
    align_apk(&opts.output).map_err(ShieldError::from)?;
    print_success("APK数据对齐完成");

    Ok(())
}

fn emit_progress<F>(
    on_progress: &F,
    cancel: &Arc<AtomicBool>,
    step: ProgressStep,
    message: &str,
) -> std::result::Result<(), ShieldError>
where
    F: Fn(ProgressEvent) + Send + 'static,
{
    check_cancel(cancel)?;
    on_progress(ProgressEvent {
        step,
        message: message.to_string(),
    });
    Ok(())
}

fn check_cancel(cancel: &Arc<AtomicBool>) -> std::result::Result<(), ShieldError> {
    if cancel.load(Ordering::Relaxed) {
        return Err(ShieldError::Cancelled);
    }

    Ok(())
}

fn print_internal_step(step: &str) {
    if !is_json_mode() {
        println!("\n{} {}", "=>".cyan().bold(), step.bold());
    }
}

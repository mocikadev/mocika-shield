use colored::Colorize;
use rand::RngCore;
use std::fs;
use std::path::PathBuf;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

use crate::apk_inspect::{
    check_apk, extract_apk_cert_fingerprint, normalize_fingerprint, ApkCheckOutcome,
};
use crate::error::ShieldError;
use crate::protect::{
    dex::process_dex,
    manifest::modify_manifest,
    runtime::{inject_runtime, read_stub_application},
};
use crate::utils::is_json_mode;
use crate::utils::{
    create_temp_dir, find_apksigner, find_apktool, find_java, find_runtime_resources, human_size,
    print_step, print_success, run_command,
};
use crate::zipalign::align_apk;

#[derive(Debug, Clone)]
pub struct ProtectOptions {
    pub input: PathBuf,
    pub output: PathBuf,
    /// 用户自定义 apktool.jar 路径（优先于自动查找）
    pub apktool_path: Option<PathBuf>,
    /// 用户自定义 resources.zip 路径（优先于自动查找）
    pub resources_path: Option<PathBuf>,
    /// 用户自定义 apksigner.jar 路径（优先于自动查找）
    pub apksigner_path: Option<PathBuf>,
    /// 计划用于加固输出签名的证书指纹；提供时必须与输入 APK 当前证书一致
    pub expected_output_cert_fingerprint: Option<String>,
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
    let apksigner = match &opts.apksigner_path {
        Some(p) if p.exists() => p.clone(),
        Some(p) => {
            return Err(ShieldError::from(anyhow::anyhow!(
                "配置的 apksigner.jar 路径不存在: {}",
                p.display()
            )))
        }
        None => find_apksigner().map_err(ShieldError::from)?,
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
    print_success("所有工具就绪");

    let apk_check = check_apk(&opts.input, Some(&apksigner)).map_err(ShieldError::from)?;
    validate_apk_eligibility(&apk_check).map_err(ShieldError::from)?;
    let signature =
        extract_apk_cert_fingerprint(&opts.input, Some(&apksigner)).map_err(ShieldError::from)?;
    validate_output_certificate(&signature, opts.expected_output_cert_fingerprint.as_deref())
        .map_err(ShieldError::from)?;
    print_success(&format!(
        "原始 APK 当前签名证书 SHA-256: {}...",
        &signature[..16]
    ));

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

fn validate_apk_eligibility(outcome: &ApkCheckOutcome) -> anyhow::Result<()> {
    if outcome.already_protected {
        anyhow::bail!("该 APK 已经加固，禁止重复加固。请使用原始未加固 APK")
    }
    if !outcome.is_signed {
        anyhow::bail!("该 APK 尚未签名，加固需要有效签名的 APK")
    }
    Ok(())
}

fn validate_output_certificate(
    input_fingerprint: &str,
    expected_output_fingerprint: Option<&str>,
) -> anyhow::Result<()> {
    let Some(expected) = expected_output_fingerprint else {
        return Ok(());
    };
    if normalize_fingerprint(input_fingerprint) != normalize_fingerprint(expected) {
        anyhow::bail!(
            "原 APK 签名证书与所选自动签名证书不一致；加固数据绑定原证书，使用所选证书签名后应用将无法启动。请选择与原 APK 相同的证书"
        )
    }
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

#[cfg(test)]
mod tests {
    use super::{validate_apk_eligibility, validate_output_certificate};
    use crate::apk_inspect::ApkCheckOutcome;

    #[test]
    fn 已加固_apk_被核心入口拒绝() {
        let outcome = ApkCheckOutcome {
            already_protected: true,
            is_signed: true,
        };
        let error = validate_apk_eligibility(&outcome).unwrap_err();
        assert!(error.to_string().contains("禁止重复加固"));
    }

    #[test]
    fn 未签名_apk_被核心入口拒绝() {
        let outcome = ApkCheckOutcome {
            already_protected: false,
            is_signed: false,
        };
        let error = validate_apk_eligibility(&outcome).unwrap_err();
        assert!(error.to_string().contains("尚未签名"));
    }

    #[test]
    fn 自动签名证书不一致时失败关闭() {
        let error = validate_output_certificate("AA:BB", Some("CCDD")).unwrap_err();
        assert!(error.to_string().contains("应用将无法启动"));
    }

    #[test]
    fn 自动签名证书比较会规范化格式() {
        validate_output_certificate("aa:bb cc", Some("AABBCC")).unwrap();
    }

    #[test]
    fn 未配置自动签名时允许生成未签名产物() {
        validate_output_certificate("AABB", None).unwrap();
    }
}

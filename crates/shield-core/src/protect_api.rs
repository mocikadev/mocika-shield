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
use crate::diagnostic::{Diagnostic, FailureCode, ToolName};
use crate::error::ShieldError;
use crate::protect::{
    abi_filter::{remove_excluded, validate_exclusions},
    dex::process_dex,
    manifest::{
        add_cache_identity, add_memory_payload_metrics, modify_manifest,
        read_native_lib_packaging_policy, NativeLibPackagingPolicy,
    },
    native_alias::verify_in_apk,
    resource_format::normalize_mislabeled_jpeg_resources,
    runtime::{inject_runtime, read_runtime_selection},
};
use crate::utils::is_json_mode;
use crate::utils::{
    create_temp_dir, find_apksigner, find_apktool, find_java, find_runtime_resources, human_size,
    no_window_command, print_step, print_success,
};
use std::process::Stdio;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum EnvironmentPolicy {
    #[default]
    Compatible,
    Strict,
}

impl EnvironmentPolicy {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Compatible => "compatible",
            Self::Strict => "strict",
        }
    }
}
use crate::zipalign::{align_apk_with_native_packaging, NativeLibraryPackaging};

#[derive(Debug, Clone)]
pub struct ProtectOptions {
    /// 用户对本次任务明确确认排除的、不受支持的 Native ABI。
    pub excluded_abis: Vec<String>,
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
    /// 运行时环境安全策略；默认兼容模式仅执行原有反调试检查。
    pub environment_policy: EnvironmentPolicy,
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
        None => find_apktool().map_err(missing_tool)?,
    };
    let custom_runtime_resources = opts.resources_path.is_some();
    let runtime_resources = match &opts.resources_path {
        Some(p) if p.exists() => p.clone(),
        Some(p) => {
            return Err(ShieldError::from(anyhow::anyhow!(
                "配置的 resources.zip 路径不存在: {}",
                p.display()
            )))
        }
        None => find_runtime_resources().map_err(missing_tool)?,
    };
    let apksigner = match &opts.apksigner_path {
        Some(p) if p.exists() => p.clone(),
        Some(p) => {
            return Err(ShieldError::from(anyhow::anyhow!(
                "配置的 apksigner.jar 路径不存在: {}",
                p.display()
            )))
        }
        None => find_apksigner().map_err(missing_tool)?,
    };
    let runtime_selection = read_runtime_selection(
        &runtime_resources,
        opts.environment_policy,
        custom_runtime_resources,
    )
    .map_err(ShieldError::from)?;

    if !is_json_mode() {
        println!("{}", "========================================".cyan());
        println!("{}", "Mocika Shield - APK Protection".cyan().bold());
        println!("{}", "========================================".cyan());
        println!("输入APK: {:?}", opts.input);
        println!("输出APK: {:?}", opts.output);
        println!("{}", "========================================".cyan());
    }

    let java = find_java().map_err(missing_tool)?;

    emit_progress(&on_progress, &cancel, ProgressStep::CheckTools, "检查工具")?;
    print_step("检查工具");
    print_success("所有工具就绪");

    let apk_check = check_apk(&opts.input, Some(&apksigner)).map_err(ShieldError::from)?;
    validate_apk_eligibility(&apk_check).map_err(preflight)?;
    validate_exclusions(&apk_check.native_abis, &opts.excluded_abis).map_err(preflight)?;
    let signature =
        extract_apk_cert_fingerprint(&opts.input, Some(&apksigner)).map_err(ShieldError::from)?;
    validate_output_certificate(&signature, opts.expected_output_cert_fingerprint.as_deref())
        .map_err(preflight)?;
    print_success(&format!(
        "原始 APK 当前签名证书 SHA-256: {}...",
        &signature[..16]
    ));

    let temp_dir = create_temp_dir("shield-").map_err(ShieldError::from)?;
    let apk_dir = temp_dir.path().join("apk");

    emit_progress(&on_progress, &cancel, ProgressStep::Unpack, "解包APK")?;
    print_step("解包APK");
    run_apktool_command(
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
        FailureCode::ToolProcessFailed,
    )
    .map_err(ShieldError::from)?;
    print_success("解包完成");
    remove_excluded(&apk_dir, &opts.excluded_abis).map_err(ShieldError::from)?;
    if !opts.excluded_abis.is_empty() {
        let message = format!("已从输出排除 ABI：{}", opts.excluded_abis.join("、"));
        emit_progress(&on_progress, &cancel, ProgressStep::Unpack, &message)?;
        print_success(&message);
    }

    let native_lib_policy =
        read_native_lib_packaging_policy(&apk_dir).map_err(ShieldError::from)?;
    let normalized_resources =
        normalize_mislabeled_jpeg_resources(&apk_dir).map_err(ShieldError::from)?;
    if normalized_resources > 0 {
        print_success(&format!(
            "已修正 {normalized_resources} 个 JPEG 内容伪装 PNG 的资源文件"
        ));
    }

    emit_progress(
        &on_progress,
        &cancel,
        ProgressStep::ModifyManifest,
        "修改AndroidManifest.xml",
    )?;
    print_step("修改AndroidManifest.xml");
    modify_manifest(
        &apk_dir,
        &runtime_selection.stub_application,
        runtime_selection.stub_component_factory.as_deref(),
        opts.environment_policy,
    )
    .map_err(ShieldError::from)?;
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
    let cache_identity = process_dex(&apk_dir, &signature, &ikm).map_err(ShieldError::from)?;
    add_cache_identity(&apk_dir, &cache_identity).map_err(ShieldError::from)?;
    if runtime_selection.memory_dex {
        add_memory_payload_metrics(&apk_dir, &cache_identity).map_err(ShieldError::from)?;
    }

    emit_progress(
        &on_progress,
        &cancel,
        ProgressStep::InjectRuntime,
        "注入Runtime库",
    )?;
    print_step("注入Runtime库");
    let injected_runtime = inject_runtime(
        &apk_dir,
        &runtime_resources,
        &opts.input,
        &opts.excluded_abis,
    )
    .map_err(|err| diagnosed(err, FailureCode::RuntimeInjectionFailed))?;
    print_success("Runtime库注入完成");

    emit_progress(&on_progress, &cancel, ProgressStep::Repack, "重打包APK")?;
    print_step("重打包APK");
    run_apktool_command(
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
        FailureCode::ApkRepackFailed,
    )
    .map_err(|err| diagnosed(err, FailureCode::ApkRepackFailed))?;

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
    align_apk_with_native_packaging(&opts.output, native_library_packaging(native_lib_policy))
        .map_err(|err| diagnosed(err, FailureCode::AlignmentFailed))?;
    verify_in_apk(&opts.output, &injected_runtime).map_err(ShieldError::from)?;
    print_success("APK数据对齐完成");

    Ok(())
}

fn run_apktool_command(
    java: &std::path::Path,
    args: &[&str],
    fallback: FailureCode,
) -> anyhow::Result<String> {
    let output = no_window_command(java)
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
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

fn apktool_command_error(
    java: &std::path::Path,
    fallback: FailureCode,
    exit_code: Option<i32>,
    stdout: &[u8],
    stderr: &[u8],
) -> anyhow::Error {
    let evidence = if stderr.is_empty() { stdout } else { stderr };
    let mut diagnostic = Diagnostic::from_tool_output(ToolName::Apktool, exit_code, evidence);
    if diagnostic.code == FailureCode::ToolProcessFailed {
        diagnostic.code = fallback;
    }
    diagnostic.attach(anyhow::anyhow!(
        "命令执行失败: {:?}\n错误: {}",
        java,
        String::from_utf8_lossy(stderr)
    ))
}

fn missing_tool(error: anyhow::Error) -> ShieldError {
    diagnosed(error, FailureCode::ToolNotFound)
}

fn diagnosed(error: anyhow::Error, fallback: FailureCode) -> ShieldError {
    ShieldError::from(
        Diagnostic::from_error(&error)
            .or_code(fallback)
            .attach(error),
    )
}

fn preflight(error: anyhow::Error) -> ShieldError {
    let diagnostic = Diagnostic::from_error(&error);
    ShieldError::from(diagnostic.attach_preflight(error))
}

fn native_library_packaging(policy: NativeLibPackagingPolicy) -> NativeLibraryPackaging {
    match policy {
        NativeLibPackagingPolicy::Disabled => NativeLibraryPackaging::Store,
        NativeLibPackagingPolicy::Enabled | NativeLibPackagingPolicy::Unspecified => {
            NativeLibraryPackaging::Preserve
        }
    }
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
    use super::{native_library_packaging, validate_apk_eligibility, validate_output_certificate};
    use crate::diagnostic::{Diagnostic, FailureCode, ToolName};

    #[test]
    fn apktool_失败在字节转换前保留工具诊断() {
        let unsupported = super::apktool_command_error(
            std::path::Path::new("java"),
            FailureCode::ApkRepackFailed,
            Some(1),
            b"",
            b"java.lang.UnsupportedClassVersionError",
        );
        assert_eq!(
            Diagnostic::from_error(&unsupported).code,
            FailureCode::JavaUnsupported
        );
        assert_eq!(
            Diagnostic::from_error(&unsupported).tool,
            Some(ToolName::Apktool)
        );
        assert_eq!(Diagnostic::from_error(&unsupported).exit_code, Some(1));
        assert!(unsupported
            .to_string()
            .contains("UnsupportedClassVersionError"));

        let invalid = super::apktool_command_error(
            std::path::Path::new("java"),
            FailureCode::ApkRepackFailed,
            Some(1),
            b"",
            b"\xff\xfe",
        );
        assert_eq!(
            Diagnostic::from_error(&invalid).code,
            FailureCode::ToolOutputEncodingInvalid
        );
        assert!(invalid.to_string().contains("命令执行失败"));

        let unpack = super::apktool_command_error(
            std::path::Path::new("java"),
            FailureCode::ToolProcessFailed,
            Some(2),
            b"",
            b"ordinary apktool failure",
        );
        let repack = super::apktool_command_error(
            std::path::Path::new("java"),
            FailureCode::ApkRepackFailed,
            Some(3),
            b"",
            b"ordinary apktool failure",
        );
        assert_eq!(
            Diagnostic::from_error(&unpack).code,
            FailureCode::ToolProcessFailed
        );
        assert_eq!(
            Diagnostic::from_error(&repack).code,
            FailureCode::ApkRepackFailed
        );
    }
    use crate::apk_inspect::ApkCheckOutcome;
    use crate::protect::manifest::NativeLibPackagingPolicy;
    use crate::zipalign::NativeLibraryPackaging;

    #[test]
    fn extract_native_libs_false_映射为不压缩策略() {
        assert_eq!(
            native_library_packaging(NativeLibPackagingPolicy::Disabled),
            NativeLibraryPackaging::Store
        );
        assert_eq!(
            native_library_packaging(NativeLibPackagingPolicy::Enabled),
            NativeLibraryPackaging::Preserve
        );
        assert_eq!(
            native_library_packaging(NativeLibPackagingPolicy::Unspecified),
            NativeLibraryPackaging::Preserve
        );
    }

    #[test]
    fn 已加固_apk_被核心入口拒绝() {
        let outcome = ApkCheckOutcome {
            already_protected: true,
            is_signed: true,
            native_abis: Vec::new(),
        };
        let error = validate_apk_eligibility(&outcome).unwrap_err();
        assert!(error.to_string().contains("禁止重复加固"));
    }

    #[test]
    fn 未签名_apk_被核心入口拒绝() {
        let outcome = ApkCheckOutcome {
            already_protected: false,
            is_signed: false,
            native_abis: Vec::new(),
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

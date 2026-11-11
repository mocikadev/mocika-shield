use colored::Colorize;
use rand::RngCore;
use std::fs;
use std::path::PathBuf;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

use crate::apk_inspect::{normalize_fingerprint, ApkCheckOutcome};
use crate::diagnostic::{Diagnostic, FailureCode};
use crate::error::ShieldError;
use crate::protect::{
    abi_filter::remove_excluded,
    class_identity::validate_class_identity,
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
use crate::utils::{find_runtime_resources, human_size, print_step, print_success};
#[path = "protect/toolchain.rs"]
mod toolchain;

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
    #[cfg(windows)]
    return protect_default(opts, on_progress, cancel);
    #[cfg(not(windows))]
    run(opts, None, on_progress, cancel)
}

#[cfg(any(windows, test))]
fn protect_default(
    opts: &ProtectOptions,
    on_progress: impl Fn(ProgressEvent) + Send + 'static,
    cancel: Arc<AtomicBool>,
) -> std::result::Result<(), ShieldError> {
    check_cancel(&cancel)?;
    check_input(&opts.input)?;
    let opts = toolchain::ordinary_options(opts).map_err(ShieldError::from)?;
    let session =
        crate::java_adapter::LocalSession::for_output(&opts.output).map_err(ShieldError::from)?;
    // 保留资源是否由用户明确指定的原语义，不把默认资源伪装成自定义覆盖。
    let result = run(
        &opts,
        Some((&session.context, &session.java)),
        on_progress,
        cancel,
    );
    session.finish(result)
}

/// 显式加固：工具及壳资源必须已选定；不在调用内重新选择工具。
/// 借用根内完成重建／对齐／壳核对，再以 rename 写回输出；同卷成功才替换已有输出。
/// 不允许覆盖输入自身，不跨卷复制回退；材料由调用方确认整组停止后回收。
/// 阶段取消不等于运行中硬超时或同步写回区间的原子取消。
pub fn protect_apk_with_context(
    context: &crate::JavaToolContext,
    java: &std::path::Path,
    opts: &ProtectOptions,
    on_progress: impl Fn(ProgressEvent) + Send + 'static,
    cancel: Arc<AtomicBool>,
) -> std::result::Result<(), ShieldError> {
    check_cancel(&cancel)?;
    let opts = toolchain::explicit_options(opts).map_err(ShieldError::from)?;
    run(&opts, Some((context, java)), on_progress, cancel)
}

fn run(
    opts: &ProtectOptions,
    adapter: Option<(&crate::JavaToolContext, &std::path::Path)>,
    on_progress: impl Fn(ProgressEvent) + Send + 'static,
    cancel: Arc<AtomicBool>,
) -> std::result::Result<(), ShieldError> {
    check_input(&opts.input)?;

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

    let tools = toolchain::Tools::new(opts, adapter).map_err(missing_tool)?;

    emit_progress(&on_progress, &cancel, ProgressStep::CheckTools, "检查工具")?;
    print_step("检查工具");
    print_success("所有工具就绪");

    let apk_check = tools.inspect(&opts.input).map_err(ShieldError::from)?;
    validate_apk_eligibility(&apk_check).map_err(preflight)?;
    runtime_selection
        .profile
        .validate_native_abis(&apk_check.native_abis, &opts.excluded_abis)
        .map_err(preflight)?;
    let signature = tools.fingerprint(&opts.input).map_err(ShieldError::from)?;
    validate_output_certificate(&signature, opts.expected_output_cert_fingerprint.as_deref())
        .map_err(preflight)?;
    print_success(&format!(
        "原始 APK 当前签名证书 SHA-256: {}...",
        &signature[..16]
    ));

    emit_progress(&on_progress, &cancel, ProgressStep::Unpack, "解包APK")?;
    print_step("解包APK");
    let project = tools.decode(&opts.input).map_err(ShieldError::from)?;
    let apk_dir = project.directory();
    print_success("解包完成");
    remove_excluded(apk_dir, &opts.excluded_abis).map_err(ShieldError::from)?;
    if !opts.excluded_abis.is_empty() {
        let message = format!("已从输出排除 ABI：{}", opts.excluded_abis.join("、"));
        emit_progress(&on_progress, &cancel, ProgressStep::Unpack, &message)?;
        print_success(&message);
    }

    let native_lib_policy = read_native_lib_packaging_policy(apk_dir).map_err(ShieldError::from)?;
    let normalized_resources =
        normalize_mislabeled_jpeg_resources(apk_dir).map_err(ShieldError::from)?;
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
        apk_dir,
        &runtime_selection.stub_application,
        runtime_selection.stub_component_factory.as_deref(),
        opts.environment_policy,
        runtime_selection.manifest_keys,
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
    validate_class_identity(apk_dir, &runtime_resources).map_err(preflight)?;
    let cache_identity = process_dex(apk_dir, &signature, &ikm).map_err(ShieldError::from)?;
    add_cache_identity(apk_dir, &cache_identity, runtime_selection.manifest_keys)
        .map_err(ShieldError::from)?;
    if runtime_selection.memory_dex {
        add_memory_payload_metrics(apk_dir, &cache_identity, runtime_selection.manifest_keys)
            .map_err(ShieldError::from)?;
    }

    emit_progress(
        &on_progress,
        &cancel,
        ProgressStep::InjectRuntime,
        "注入Runtime库",
    )?;
    print_step("注入Runtime库");
    let injected_runtime = inject_runtime(
        apk_dir,
        &runtime_resources,
        &opts.input,
        &opts.excluded_abis,
    )
    .map_err(|err| diagnosed(err, FailureCode::RuntimeInjectionFailed))?;
    print_success("Runtime库注入完成");

    emit_progress(&on_progress, &cancel, ProgressStep::Repack, "重打包APK")?;
    print_step("重打包APK");
    let candidate = tools
        .rebuild(&project, &opts.output)
        .map_err(|err| diagnosed(err, FailureCode::ApkRepackFailed))?;

    let input_size = fs::metadata(&opts.input)
        .map_err(anyhow::Error::from)
        .map_err(ShieldError::from)?
        .len();
    let output_size = fs::metadata(&candidate)
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
    align_apk_with_native_packaging(&candidate, native_library_packaging(native_lib_policy))
        .map_err(|err| diagnosed(err, FailureCode::AlignmentFailed))?;
    verify_in_apk(&candidate, &injected_runtime).map_err(ShieldError::from)?;
    print_success("APK数据对齐完成");

    if adapter.is_some() {
        check_cancel(&cancel)?;
        // 失败不先删旧输出，也不做跨卷复制；公开受控任务的最终无覆盖提交仍由外层负责。
        fs::rename(&candidate, &opts.output).map_err(|error| {
            ShieldError::from(Diagnostic::from_io(&error).attach(anyhow::anyhow!(
                "加固候选写回失败；请选同卷输出，候选保留，未删除已有输出或跨卷重试。"
            )))
        })?;
    }

    Ok(())
}

fn check_input(path: &std::path::Path) -> Result<(), ShieldError> {
    if !path.exists() {
        return Err(ShieldError::FileNotFound(path.display().to_string()));
    }
    Ok(())
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
#[path = "protect/api_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "protect/context_tests.rs"]
mod context_tests;

#[cfg(test)]
#[path = "protect/context_assertions.rs"]
mod context_assertions;
#[cfg(test)]
#[path = "protect/context_fixture.rs"]
mod context_fixture;
#[cfg(test)]
#[path = "protect/ordinary_tests.rs"]
mod ordinary_apk_tests;

use super::{require_file, CheckError, CheckResponse, CheckResult, PreflightArgs, Status};
use serde_json::json;
use shield_core::utils::{find_apksigner, probe_java_environment, strip_unc_prefix};
use shield_core::{preflight_apk, PreflightOptions, PreflightReport, PreflightSeverity};

pub(super) fn check(args: PreflightArgs) -> CheckResponse {
    let input = strip_unc_prefix(&args.input);
    require_file(&input, "INPUT_NOT_FILE")?;
    let java = probe_java_environment();
    if !java.java_ready() {
        let code = if java.java_path.is_none() {
            "TOOL_NOT_FOUND"
        } else if java.major_version.is_none() {
            "JAVA_PROBE_FAILED"
        } else {
            "JAVA_UNSUPPORTED"
        };
        return Err(CheckError::new(
            code,
            "APK 预检需要可执行的 Java 8 及以上运行环境。",
        ));
    }
    let signer = args
        .apksigner
        .map(Ok)
        .unwrap_or_else(find_apksigner)
        .map_err(|_| CheckError::new("TOOL_NOT_FOUND", "APK 预检需要可读的 apksigner.jar。"))?;
    let signer = strip_unc_prefix(&signer);
    require_file(&signer, "TOOL_NOT_FILE")?;
    let (profile, profile_name) = args.runtime_profile.resolve();
    let report = preflight_apk(
        &input,
        PreflightOptions {
            runtime_profile: profile,
            apksigner_path: Some(&signer),
            expected_output_cert_fingerprint: args.expected_cert_sha256.as_deref(),
        },
    )
    .map_err(CheckError::from_core)?;
    Ok(CheckResult {
        status: status(report.verdict),
        data: report_data(report, profile_name)?,
    })
}

pub(super) fn report_data(
    report: PreflightReport,
    profile_name: &str,
) -> Result<serde_json::Value, CheckError> {
    // ABI 名称来自输入，限制响应规模；不截断后再给出不完整的通过结论。
    if report.facts.native_abis.len() > 64
        || report.facts.native_abis.iter().any(|abi| abi.len() > 64)
    {
        return Err(CheckError::new(
            "INPUT_LIMIT_EXCEEDED",
            "APK 架构条目超出自动化检查的响应限制，请在本机人工检查。",
        ));
    }
    let binding_checked = report
        .checks
        .iter()
        .any(|check| matches!(check.code, "certificate" | "certificate_mismatch"));
    let checks: Vec<_> = report
        .checks
        .iter()
        .map(|check| {
            // 布尔验签失败也可能来自工具异常，不向 Agent 断言“原包一定未签名”。
            let code = if check.code == "unsigned" {
                "signature_not_verified"
            } else {
                check.code
            };
            json!({
                "code": code,
                "status": status(check.severity),
            })
        })
        .collect();
    Ok(json!({
        "scope": "apk_preflight",
        "runtime_profile": profile_name,
        "certificate_binding_checked": binding_checked,
        "checks": checks,
        "facts": {
            "apk_size": report.facts.apk_size,
            "dex_count": report.facts.dex_count,
            "dex_total_size": report.facts.dex_total_size,
            "native_library_count": report.facts.native_library_count,
            "compressed_native_library_count": report.facts.compressed_native_library_count,
            "native_abis": report.facts.native_abis,
        },
    }))
}

pub(super) fn status(value: PreflightSeverity) -> Status {
    match value {
        PreflightSeverity::Ready => Status::Ready,
        PreflightSeverity::Warning => Status::Warning,
        PreflightSeverity::Blocked => Status::Blocked,
    }
}

//! 加固后签名的只读计划；返回摘要而非执行授权，不创建加固产物或保存凭据。
use super::preflight::{report_data, status};
use super::resources;
use super::snapshot::{self, FileSnapshot};
use super::{CheckError, CheckResponse, CheckResult, PlanProtectArgs, Status};
use crate::args::{EnvironmentPolicyArg, KeystoreTypeArg, ProtectArgs, SignArgs};
use crate::config::{CliConfig, ResolvedProtectArgs, ResolvedSignArgs};
use serde_json::json;
use sha2::{Digest, Sha256};
use shield_core::utils::{ensure_keytool, find_apksigner, find_apktool};
use shield_core::{EnvironmentPolicy, PreflightOptions, RuntimeProfile};
use std::path::{Path, PathBuf};

mod sources;
pub(super) use sources::Sources;

pub(super) struct PreparedPlan {
    pub result: CheckResult,
    #[cfg(all(
        feature = "agent-executor",
        any(target_os = "macos", target_os = "linux", windows)
    ))]
    pub sources: Sources,
    #[cfg(all(
        feature = "agent-executor",
        any(target_os = "macos", target_os = "linux", windows)
    ))]
    pub protect: ResolvedProtectArgs,
    #[cfg(all(
        feature = "agent-executor",
        any(target_os = "macos", target_os = "linux", windows)
    ))]
    pub sign: ResolvedSignArgs,
    #[cfg(all(
        feature = "agent-executor",
        any(target_os = "macos", target_os = "linux", windows)
    ))]
    pub profile: RuntimeProfile,
    #[cfg(all(
        feature = "agent-executor",
        any(target_os = "macos", target_os = "linux", windows)
    ))]
    pub certificate: String,
    #[cfg(all(
        feature = "agent-executor",
        any(target_os = "macos", target_os = "linux", windows)
    ))]
    pub digest: [u8; 32],
}

pub(super) enum Assessment {
    Blocked(CheckResult),
    Prepared(Box<PreparedPlan>),
}

pub(super) fn check(args: PlanProtectArgs, config: Option<&Path>) -> CheckResponse {
    match prepare(args, config, false, None)? {
        Assessment::Blocked(result) => Ok(result),
        Assessment::Prepared(plan) => Ok(plan.result),
    }
}

pub(super) fn prepare(
    args: PlanProtectArgs,
    config: Option<&Path>,
    executable: bool,
    workspace: Option<&Path>,
) -> Result<Assessment, CheckError> {
    let config = config.ok_or_else(|| {
        CheckError::new(
            "SIGNING_CONFIG_REQUIRED",
            "规划需要显式指定本地 --config，不会读取 GUI 签名资料。",
        )
    })?;
    let config_snapshot = snapshot::file("config", config)?;
    // 相对配置路径沿用调用者提供配置位置的语义，不能因配置是链接而改用目标目录。
    let (protect, sign) = resolve(&args, &config_snapshot.lookup_path)?;
    check_executable_signing(&sign, executable)?;
    // 保持旧分支的早期拒绝顺序：无效输出／V4 不能因缺少 Java 被改报工具错误。
    outputs(&args.output, sign.v4)?;
    let (profile, profile_name) = args.runtime_profile.resolve();
    let sources = sources(Some(config_snapshot), &protect, &sign, profile, workspace)?;
    prepare_resolved(
        args,
        protect,
        sign,
        sources,
        executable,
        profile_name,
        workspace,
    )
}

pub(super) fn prepare_resolved(
    args: PlanProtectArgs,
    protect: ResolvedProtectArgs,
    sign: ResolvedSignArgs,
    sources: Sources,
    executable: bool,
    profile_name: &str,
    workspace: Option<&Path>,
) -> Result<Assessment, CheckError> {
    check_executable_signing(&sign, executable)?;
    let (profile, _) = args.runtime_profile.resolve();
    #[cfg(all(
        feature = "agent-executor",
        any(target_os = "macos", target_os = "linux", windows)
    ))]
    let schema = if sources.java_adapter.is_some() {
        5
    } else if sources.shared.is_some() {
        3
    } else {
        2
    };
    #[cfg(not(all(
        feature = "agent-executor",
        any(target_os = "macos", target_os = "linux", windows)
    )))]
    let schema = 2;
    #[cfg(all(
        feature = "agent-executor",
        any(target_os = "macos", target_os = "linux", windows)
    ))]
    let bound = sources
        .java_adapter
        .as_ref()
        .map(|binding| {
            super::adapter::Bound::new(
                binding,
                &binding.snapshot().path,
                workspace.ok_or_else(super::adapter::invalid)?,
            )
        })
        .transpose()?;
    #[cfg(all(
        feature = "agent-executor",
        any(target_os = "macos", target_os = "linux", windows)
    ))]
    let context = bound.as_ref().map(|b| &b.context);
    #[cfg(not(all(
        feature = "agent-executor",
        any(target_os = "macos", target_os = "linux", windows)
    )))]
    let context = {
        let _ = workspace;
        None
    };
    let tools = super::java_tools::Tools::new(&sources.java.path, context);
    let outputs = outputs(&args.output, sign.v4)?;
    let certificate = tools
        .certificate(
            &sources.keystore.path,
            &sign.key_alias,
            &sign.keystore_password,
            keystore_type(sign.keystore_type),
        )
        .map_err(CheckError::from_core)?;
    let report = tools
        .preflight(
            &sources.input.path,
            PreflightOptions {
                runtime_profile: profile,
                apksigner_path: Some(&sources.apksigner.path),
                expected_output_cert_fingerprint: Some(&certificate),
            },
        )
        .map_err(CheckError::from_core)?;
    let verdict = status(report.verdict);
    let native_abis = report.facts.native_abis.clone();
    let preflight = report_data(report, profile_name)?;
    let mut data = json!({
        "scope": "protect_sign_plan",
        "plan_schema_version": schema,
        "runtime_profile": profile_name,
        "execution_ready": false,
        "private_key_access_checked": false,
        "resource_protocol_checked": false,
        "preflight": preflight,
    });
    if verdict == Status::Blocked {
        return Ok(Assessment::Blocked(CheckResult {
            status: verdict,
            data,
        }));
    }
    profile
        .validate_native_abis(&native_abis, &protect.excluded_abis)
        .map_err(CheckError::from_core)?;
    let environment_policy = match protect.environment_policy {
        EnvironmentPolicyArg::Compatible => EnvironmentPolicy::Compatible,
        EnvironmentPolicyArg::Strict => EnvironmentPolicy::Strict,
    };
    resources::validate(
        &sources.resources.path,
        profile,
        environment_policy,
        &native_abis,
        &protect.excluded_abis,
    )?;
    data["resource_protocol_checked"] = json!(true);
    sources.recheck()?;
    if outputs != self::outputs(&args.output, sign.v4)? {
        return Err(CheckError::new(
            "PLAN_OUTPUT_CHANGED",
            "规划期间输出父位置发生变化，请重新检查。",
        ));
    }
    let policy = match protect.environment_policy {
        EnvironmentPolicyArg::Compatible => "compatible",
        EnvironmentPolicyArg::Strict => "strict",
    };
    // 只将白名单字段纳入摘要，禁止序列化含密码的 ResolvedSignArgs。
    let identity = json!({
        "plan_schema_version": schema,
        "tool_version": env!("CARGO_PKG_VERSION"),
        "operation": "protect_and_sign",
        "sources": sources,
        "outputs": outputs,
        "runtime_profile": profile_name,
        "environment_policy": policy,
        "excluded_abis": protect.excluded_abis,
        "signing": {
            "alias": sign.key_alias,
            "keystore_type": keystore_type(sign.keystore_type),
            "certificate_sha256": certificate,
            "v1": sign.v1, "v2": sign.v2, "v3": sign.v3, "v4": sign.v4,
        },
    });
    let bytes = serde_json::to_vec(&identity)
        .map_err(|_| CheckError::new("PLAN_SERIALIZATION_FAILED", "计划无法安全生成摘要。"))?;
    let digest = Sha256::digest(bytes);
    data["plan_digest"] = json!(format!("{digest:x}"));
    data["input_sha256"] = json!(sources.input.sha256);
    data["output_policy"] = json!("create_new");
    data["output_count"] = json!(outputs.len());
    data["environment_policy"] = json!(policy);
    data["excluded_abis"] = json!(protect.excluded_abis);
    Ok(Assessment::Prepared(Box::new(PreparedPlan {
        result: CheckResult {
            status: verdict,
            data,
        },
        #[cfg(all(
            feature = "agent-executor",
            any(target_os = "macos", target_os = "linux", windows)
        ))]
        sources,
        #[cfg(all(
            feature = "agent-executor",
            any(target_os = "macos", target_os = "linux", windows)
        ))]
        protect,
        #[cfg(all(
            feature = "agent-executor",
            any(target_os = "macos", target_os = "linux", windows)
        ))]
        sign,
        #[cfg(all(
            feature = "agent-executor",
            any(target_os = "macos", target_os = "linux", windows)
        ))]
        profile,
        #[cfg(all(
            feature = "agent-executor",
            any(target_os = "macos", target_os = "linux", windows)
        ))]
        certificate,
        #[cfg(all(
            feature = "agent-executor",
            any(target_os = "macos", target_os = "linux", windows)
        ))]
        digest: digest.into(),
    })))
}

fn check_executable_signing(sign: &ResolvedSignArgs, executable: bool) -> Result<(), CheckError> {
    if executable && sign.v4 {
        Err(CheckError::new(
            "SIGNING_V4_NOT_SUPPORTED",
            "受控执行首版不支持 V4，请显式选择 V1–V3；不会静默关闭 V4。",
        ))
    } else {
        Ok(())
    }
}

fn resolve(
    args: &PlanProtectArgs,
    config: &Path,
) -> Result<(ResolvedProtectArgs, ResolvedSignArgs), CheckError> {
    let config = CliConfig::load(Some(config)).map_err(|_| {
        CheckError::new(
            "CONFIG_INVALID",
            "本地 CLI 配置无效，请在本机检查，不会回传配置原文。",
        )
    })?;
    let protect = config
        .merge_protect(ProtectArgs {
            input: Some(args.input.clone()),
            output: Some(args.output.clone()),
            exclude_abis: args.exclude_abis.clone(),
            ..Default::default()
        })
        .map_err(|_| CheckError::new("CONFIG_INVALID", "加固计划配置无效。"))?;
    let password = std::env::var("MOCIKA_SHIELD_KS_PASS")
        .ok()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            CheckError::new(
                "CREDENTIAL_UNAVAILABLE",
                "宿主尚未注入可用签名凭据，请在本机配置；不要在对话或参数中提供密码。",
            )
        })?;
    let key_password = match std::env::var("MOCIKA_SHIELD_KEY_PASS") {
        Ok(value) if !value.is_empty() => Some(value),
        Err(std::env::VarError::NotPresent) => None,
        _ => {
            return Err(CheckError::new(
                "CREDENTIAL_UNAVAILABLE",
                "宿主 Key 凭据无效，请在本机检查。",
            ))
        }
    };
    let sign = config
        .merge_sign(SignArgs {
            input: Some(args.input.clone()),
            output: Some(args.output.clone()),
            ks_pass: Some(password),
            key_pass: key_password,
            ..Default::default()
        })
        .map_err(|_| CheckError::new("SIGNING_CONFIG_INVALID", "本地签名材料配置不完整或无效。"))?;
    if !(sign.v1 || sign.v2 || sign.v3) {
        return Err(CheckError::new(
            "SIGNING_VERSIONS_INVALID",
            "至少需要一种 V1、V2 或 V3 签名，V4 不能单独使用。",
        ));
    }
    Ok((protect, sign))
}

pub(super) fn sources(
    config: Option<FileSnapshot>,
    protect: &ResolvedProtectArgs,
    sign: &ResolvedSignArgs,
    profile: RuntimeProfile,
    workspace: Option<&Path>,
) -> Result<Sources, CheckError> {
    let _ = workspace;
    let java = ensure_keytool().map_err(|_| {
        CheckError::new(
            "DEPENDENCY_NOT_READY",
            "规划需要可用的 Java 和 keytool，请先运行 doctor 检查。",
        )
    })?;
    let signer = locate(
        sign.apksigner.clone().or_else(|| protect.apksigner.clone()),
        find_apksigner,
    )?;
    if let (Some(first), Some(second)) = (&protect.apksigner, &sign.apksigner) {
        let first = snapshot::file("apksigner", first)?;
        let second = snapshot::file("apksigner", second)?;
        if first.path != second.path {
            return Err(CheckError::new(
                "INCONSISTENT_TOOL_CONFIG",
                "加固与签名指定了不同的 apksigner，请在本机统一后再规划。",
            ));
        }
    }
    Ok(Sources {
        config,
        #[cfg(all(
            feature = "agent-executor",
            any(target_os = "macos", target_os = "linux", windows)
        ))]
        shared: None,
        input: snapshot::file("input", &protect.input)?,
        keystore: snapshot::file("keystore", &sign.keystore)?,
        java: snapshot::file("java", java.java_path.as_deref().ok_or_else(missing_tool)?)?,
        keytool: snapshot::file(
            "keytool",
            java.keytool_path.as_deref().ok_or_else(missing_tool)?,
        )?,
        apktool: snapshot::file("apktool", &locate(protect.apktool.clone(), find_apktool)?)?,
        apksigner: snapshot::file("apksigner", &signer)?,
        resources: snapshot::file(
            "resources",
            &resources::select(protect.resources.clone(), profile)?,
        )?,
        #[cfg(all(
            feature = "agent-executor",
            any(target_os = "macos", target_os = "linux", windows)
        ))]
        java_adapter: {
            #[cfg(windows)]
            {
                workspace.map(|_| super::adapter::select()).transpose()?
            }
            #[cfg(not(windows))]
            {
                let _ = workspace;
                None
            }
        },
    })
}

fn outputs(path: &Path, v4: bool) -> Result<Vec<PathBuf>, CheckError> {
    if !path
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("apk"))
    {
        return Err(CheckError::new(
            "OUTPUT_PATH_INVALID",
            "计划的最终输出必须是 APK 文件。",
        ));
    }
    let mut paths = vec![snapshot::output(path)?];
    if v4 {
        let mut sidecar = path.as_os_str().to_os_string();
        sidecar.push(".idsig");
        paths.push(snapshot::output(Path::new(&sidecar))?);
    }
    Ok(paths)
}

fn locate(
    path: Option<PathBuf>,
    find: impl FnOnce() -> anyhow::Result<PathBuf>,
) -> Result<PathBuf, CheckError> {
    path.map(Ok)
        .unwrap_or_else(find)
        .map_err(|_| missing_tool())
}

fn missing_tool() -> CheckError {
    CheckError::new(
        "TOOL_NOT_FOUND",
        "计划所需工具或资源不可用，请先运行 doctor 检查。",
    )
}

pub(super) fn keystore_type(kind: KeystoreTypeArg) -> &'static str {
    match kind {
        KeystoreTypeArg::Jks => "JKS",
        KeystoreTypeArg::Pkcs12 => "PKCS12",
    }
}

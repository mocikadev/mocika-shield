//! 工具参数到固定 CLI 调用的映射；不持有执行器状态或实现加固规则。
use super::{
    bridge::Invocation,
    policy::{Policy, Result},
    protocol::hex_digest,
};
use rmcp::model::{Tool, ToolAnnotations};
use serde::Deserialize;
use serde_json::{json, Value};
use std::ffi::OsString;
mod certificates;
mod management;
pub(super) use management::{management_call, management_tool};

#[derive(Default, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum Operation {
    #[default]
    Protect,
    Sign,
    Certificate,
}
impl Operation {
    fn name(&self) -> &'static str {
        match self {
            Self::Protect => "protect",
            Self::Sign => "sign",
            Self::Certificate => "certificate",
        }
    }
}
#[derive(Default, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum Profile {
    #[default]
    Standard,
    AndroidApi19,
}
impl Profile {
    fn name(&self) -> &'static str {
        match self {
            Self::Standard => "standard",
            Self::AndroidApi19 => "android-api19",
        }
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Doctor {
    #[serde(default)]
    operation: Operation,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Preflight {
    input: String,
    #[serde(default)]
    runtime_profile: Profile,
    expected_cert_sha256: Option<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Plan {
    input: String,
    output: String,
    #[serde(default)]
    runtime_profile: Option<Profile>,
    certificate_id: Option<String>,
    #[serde(default)]
    exclude_abis: Vec<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Sign {
    pub input: String,
    pub output: String,
    pub certificate_id: Option<String>,
}

pub(super) fn list(allow_execution: bool, shared: bool) -> Vec<Tool> {
    let relative = json!({"type":"string","minLength":1,"maxLength":2048,"description":"启动时授权根目录内的相对路径，不接受链接或父级跳转"});
    let profile = json!({"type":"string","enum":["standard","android-api19"],"default":"standard"});
    let mut tools = vec![
        tool(
            "shield_doctor",
            "只读检查默认工具和 Java 环境；不修改系统环境，不代表签名配置就绪。",
            json!({
                "operation":{"type":"string","enum":["protect","sign","certificate"],"default":"protect"}
            }),
            &[],
        ),
        tool(
            "shield_preflight",
            "只读检查授权 APK，返回有限风险事实；不加固、不自动排除架构。",
            json!({
                "input":relative,"runtime_profile":profile,"expected_cert_sha256":{"type":"string","pattern":"^[a-fA-F0-9]{64}$"}
            }),
            &["input"],
        ),
        tool(
            "shield_plan",
            "受控规划加固后签名，不生成 APK；使用宿主固定配置和凭据，返回本会话内限时计划引用；不代表已获执行授权。",
            json!({
                "input":relative,"output":relative,"runtime_profile":profile,
                "exclude_abis":{"type":"array","maxItems":16,"uniqueItems":true,"items":{"type":"string","maxLength":64}}
            }),
            &["input", "output"],
        ),
    ];
    if shared {
        let id = json!({"type":"string","minLength":1,"maxLength":128,"description":"宿主白名单中的证书编号，不接受路径或 Alias"});
        tools.push(tool("shield_certificates","仅查询宿主白名单内的证书编号、默认标记和签名版本；不返回名称、路径、Alias、备注或密码。",
            json!({"certificate_id":id}),&[]));
        let plan = tools
            .iter_mut()
            .find(|tool| tool.name == "shield_plan")
            .unwrap();
        plan.description=Some("使用宿主固定共享上下文及白名单证书规划加固签名，不生成 APK；必须明确证书编号，返回会话内计划引用。".into());
        plan.input_schema=std::sync::Arc::new(json!({"type":"object","additionalProperties":false,
            "properties":{"input":relative,"output":relative,"runtime_profile":{"type":"string","enum":["standard","android-api19"]},
                "certificate_id":id,"exclude_abis":{"type":"array","maxItems":16,"uniqueItems":true,"items":{"type":"string","maxLength":64}}},
            "required":["input","output","certificate_id"]}).as_object().unwrap().clone());
    }
    if allow_execution {
        let id = json!({"type":"string","pattern":"^[a-f0-9]{32}$"});
        let mut protect = tool("shield_protect", "使用本会话计划及匹配摘要加固签名；调用前须取得用户授权。开始尝试即消费计划，不覆盖、不自动重试，不安装或发布 APK。",
            json!({"plan_id":id,"expect_plan_sha256":{"type":"string","pattern":"^[a-fA-F0-9]{64}$"}}), &["plan_id","expect_plan_sha256"]);
        protect.annotations = Some(
            ToolAnnotations::new()
                .read_only(false)
                .idempotent(false)
                .open_world(false),
        );
        tools.push(protect);
        if shared {
            let mut sign = tool("shield_sign", "使用共享证书独立签名，一次调用自动规划和交付，不加固、不覆盖、不安装。省略编号时使用范围内默认项或唯一允许项，无额外确认。返回的 plan_id 仅供原状态工具查询，不是签名前置计划。",
                json!({"input":relative,"output":relative,"certificate_id":{"type":"string","minLength":1,"maxLength":128}}), &["input","output"]);
            sign.annotations = Some(
                ToolAnnotations::new()
                    .read_only(false)
                    .destructive(false)
                    .idempotent(false)
                    .open_world(false),
            );
            tools.push(sign);
        }
        tools.push(tool(
            "shield_execution_status",
            "只读查询本会话计划的执行阶段与终态证据；不重新检查环境、不启动或恢复任务。",
            json!({"plan_id":id}),
            &["plan_id"],
        ));
    }
    tools.extend(management::list(allow_execution, shared));
    if allow_execution && shared {
        tools.extend(certificates::list());
    }
    tools
}
fn tool(
    name: &'static str,
    description: &'static str,
    properties: Value,
    required: &[&str],
) -> Tool {
    let schema = json!({"type":"object","properties":properties,"required":required,"additionalProperties":false});
    let mut tool = Tool::new(name, description, schema.as_object().unwrap().clone());
    tool.annotations = Some(ToolAnnotations::new().read_only(true).open_world(false));
    tool
}

pub(super) fn invocation(name: &str, arguments: Value, policy: &Policy) -> Result<Invocation> {
    let invalid = |_| "ARGUMENT_INVALID";
    let (arguments, operation, controlled) = match name {
        "shield_doctor" => {
            let args: Doctor = serde_json::from_value(arguments).map_err(invalid)?;
            (
                vec![
                    "doctor".into(),
                    "--operation".into(),
                    args.operation.name().into(),
                ],
                "doctor",
                false,
            )
        }
        "shield_preflight" => {
            let args: Preflight = serde_json::from_value(arguments).map_err(invalid)?;
            let mut command = vec![
                "preflight".into(),
                "--input".into(),
                policy.input(&args.input)?.into_os_string(),
                "--runtime-profile".into(),
                args.runtime_profile.name().into(),
            ];
            if let Some(fingerprint) = args.expected_cert_sha256 {
                if !hex_digest(&fingerprint) {
                    return Err("ARGUMENT_INVALID");
                }
                command.extend(["--expected-cert-sha256".into(), fingerprint.into()]);
            }
            (command, "preflight", false)
        }
        "shield_plan" => {
            if !policy.is_shared() && arguments.get("certificate_id").is_some() {
                return Err("ARGUMENT_INVALID");
            }
            let args: Plan = serde_json::from_value(arguments).map_err(invalid)?;
            return plan_call(args, policy, None, None);
        }
        "shield_certificates" if policy.is_shared() => {
            certificate_reference(arguments, policy)?;
            let mut args = policy.shared_arguments()?;
            args.extend(["cert".into(), "list".into()]);
            return Ok(Invocation {
                arguments: args,
                operation: "cert",
                controlled: false,
                host_credentials: false,
            });
        }
        _ => return Err("TOOL_NOT_AVAILABLE"),
    };
    Ok(Invocation {
        arguments,
        operation,
        controlled,
        host_credentials: false,
    })
}

fn plan_call(
    args: Plan,
    policy: &Policy,
    digest: Option<&str>,
    binding: Option<&str>,
) -> Result<Invocation> {
    let input = policy.input(&args.input)?;
    let output = policy.output(&args.output)?;
    if args.exclude_abis.len() > 16 {
        return Err("ARGUMENT_INVALID");
    }
    let mut command: Vec<OsString> = if policy.is_shared() {
        policy.certificate(
            args.certificate_id
                .as_deref()
                .ok_or("CERTIFICATE_REQUIRED")?,
        )?;
        policy.shared_arguments()?
    } else {
        if args.certificate_id.is_some() || binding.is_some() {
            return Err("ARGUMENT_INVALID");
        }
        policy.config()?;
        Vec::new()
    };
    if let Some(config) = policy.project_config() {
        command.extend(["--config".into(), config.into()]);
    }
    command.extend([
        "agent".into(),
        if digest.is_some() { "protect" } else { "plan" }.into(),
        "--input".into(),
        input.into(),
        "--output".into(),
        output.into(),
        "--control-stdin".into(),
        "--timeout-seconds".into(),
        policy.timeout_seconds.to_string().into(),
    ]);
    if policy.is_shared() {
        if digest.is_some() {
            let binding = binding.ok_or("CLI_PROTOCOL_INVALID")?;
            let id = crate::automation::execution::shared_binding_certificate(binding)?;
            if Some(id.as_str()) != args.certificate_id.as_deref() {
                return Err("CLI_PROTOCOL_INVALID");
            }
            policy.certificate(&id)?;
            command.extend(["--shared-binding".into(), binding.into()]);
        } else {
            command.extend([
                "--use-shared-settings".into(),
                format!("--certificate-id={}", args.certificate_id.as_ref().unwrap()).into(),
            ]);
            if let Some(profile) = args.runtime_profile {
                command.extend(["--runtime-profile".into(), profile.name().into()]);
            }
        }
    } else {
        command.extend([
            "--runtime-profile".into(),
            args.runtime_profile.unwrap_or_default().name().into(),
        ]);
    }
    let mut seen = std::collections::HashSet::new();
    for abi in args.exclude_abis {
        if abi.is_empty()
            || abi.len() > 64
            || !abi
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b"-_".contains(&c))
            || !seen.insert(abi.clone())
        {
            return Err("ARGUMENT_INVALID");
        }
        // 使用等号绑定参数值，即使原 ABI 名以短横线开头也不能变成 CLI 开关。
        command.push(format!("--exclude-abis={abi}").into());
    }
    if let Some(digest) = digest {
        if !hex_digest(digest) {
            return Err("ARGUMENT_INVALID");
        }
        command.extend(["--expect-plan-sha256".into(), digest.into()]);
    }
    Ok(Invocation {
        arguments: command,
        operation: if digest.is_some() {
            "agent_protect"
        } else {
            "agent_plan"
        },
        controlled: true,
        host_credentials: false,
    })
}

pub(super) fn sign_arguments(arguments: Value, policy: &Policy) -> Result<Sign> {
    if !policy.is_shared() {
        return Err("TOOL_NOT_AVAILABLE");
    }
    let args: Sign = serde_json::from_value(arguments).map_err(|_| "ARGUMENT_INVALID")?;
    policy.input(&args.input)?;
    policy.output(&args.output)?;
    if let Some(id) = &args.certificate_id {
        policy.certificate(id)?;
    }
    Ok(args)
}
pub(super) fn sign_call(args: &Sign, policy: &Policy, certificate: &str) -> Result<Invocation> {
    policy.certificate(certificate)?;
    let mut command = policy.shared_arguments()?;
    if let Some(config) = policy.project_config() {
        command.extend(["--config".into(), config.into()]);
    }
    command.extend([
        "agent".into(),
        "sign".into(),
        "--input".into(),
        policy.input(&args.input)?.into_os_string(),
        "--output".into(),
        policy.output(&args.output)?.into_os_string(),
        "--control-stdin".into(),
        "--timeout-seconds".into(),
        policy.timeout_seconds.to_string().into(),
        format!("--certificate-id={certificate}").into(),
    ]);
    Ok(Invocation {
        arguments: command,
        operation: "agent_sign",
        controlled: true,
        host_credentials: false,
    })
}

pub(super) fn protect(
    arguments: Value,
    policy: &Policy,
    digest: &str,
    binding: Option<&str>,
) -> Result<Invocation> {
    let args = serde_json::from_value(arguments).map_err(|_| "ARGUMENT_INVALID")?;
    plan_call(args, policy, Some(digest), binding)
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CertificateReference {
    certificate_id: Option<String>,
}
pub(super) fn certificate_reference(arguments: Value, policy: &Policy) -> Result<Option<String>> {
    let reference: CertificateReference =
        serde_json::from_value(arguments).map_err(|_| "ARGUMENT_INVALID")?;
    if let Some(id) = &reference.certificate_id {
        policy.certificate(id)?;
    }
    Ok(reference.certificate_id)
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ProtectReference {
    plan_id: String,
    expect_plan_sha256: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StatusReference {
    plan_id: String,
}
fn valid_id(id: &str) -> bool {
    id.len() == 32
        && id
            .bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
}
pub(super) fn protect_reference(arguments: Value) -> Result<(String, String)> {
    let args: ProtectReference =
        serde_json::from_value(arguments).map_err(|_| "ARGUMENT_INVALID")?;
    if !valid_id(&args.plan_id) || !hex_digest(&args.expect_plan_sha256) {
        return Err("ARGUMENT_INVALID");
    }
    Ok((args.plan_id, args.expect_plan_sha256.to_ascii_lowercase()))
}
pub(super) fn status_reference(arguments: Value) -> Result<String> {
    let args: StatusReference =
        serde_json::from_value(arguments).map_err(|_| "ARGUMENT_INVALID")?;
    if !valid_id(&args.plan_id) {
        return Err("ARGUMENT_INVALID");
    }
    Ok(args.plan_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn 共享编号即使以开关开头也只作为单个参数值() {
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("输入");
        let output = dir.path().join("输出");
        std::fs::create_dir(&input).unwrap();
        std::fs::create_dir(&output).unwrap();
        std::fs::write(input.join("a.apk"), b"fixture").unwrap();
        let cli = dir.path().join("shield");
        std::fs::write(&cli, b"fixture").unwrap();
        let home = dir.path().join("缺失共享资料");
        let policy = Policy::new(&input, &output, None, &cli, 10)
            .unwrap()
            .with_shared(Some(&home), vec!["--help".into()])
            .unwrap();
        let call = invocation(
            "shield_plan",
            json!({"input":"a.apk","output":"b.apk","certificate_id":"--help"}),
            &policy,
        )
        .unwrap();
        assert!(call
            .arguments
            .contains(&OsString::from("--certificate-id=--help")));
        assert!(!call.arguments.contains(&OsString::from("--help")));
        let sign_args = sign_arguments(
            json!({"input":"a.apk","output":"b.apk","certificate_id":"--help"}),
            &policy,
        )
        .unwrap();
        let signing = sign_call(&sign_args, &policy, "--help").unwrap();
        assert_eq!(signing.operation, "agent_sign");
        assert!(signing
            .arguments
            .contains(&OsString::from("--certificate-id=--help")));
        for forbidden in [
            "--help",
            "--shared-binding",
            "--expect-plan-sha256",
            "--runtime-profile",
        ] {
            assert!(!signing.arguments.contains(&OsString::from(forbidden)));
        }
        assert!(
            !call.arguments.contains(&OsString::from("standard")),
            "缺省不能覆盖 GUI 共享模式"
        );
        assert!(invocation("shield_plan",json!({"input":"a.apk","output":"b.apk","certificate_id":"--help","shared_binding":"forged"}),&policy).is_err());
        assert!(!home.exists());
    }
    #[test]
    fn 架构名作为单个参数值传递且不能注入命令开关() {
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("输入");
        let output = dir.path().join("输出");
        std::fs::create_dir(&input).unwrap();
        std::fs::create_dir(&output).unwrap();
        std::fs::write(input.join("a.apk"), b"fixture").unwrap();
        let config = dir.path().join("shield-cli.toml");
        let cli = dir.path().join("shield");
        std::fs::write(&config, b"fixture").unwrap();
        std::fs::write(&cli, b"fixture").unwrap();
        let policy = Policy::new(&input, &output, Some(&config), &cli, 10).unwrap();
        let call = invocation(
            "shield_plan",
            json!({"input":"a.apk","output":"b.apk",
            "runtime_profile":"android-api19","exclude_abis":["--help"]}),
            &policy,
        )
        .unwrap();
        assert!(call
            .arguments
            .contains(&OsString::from("--exclude-abis=--help")));
        assert!(!call.arguments.contains(&OsString::from("--help")));
        assert!(call.arguments.contains(&OsString::from("android-api19")));
        assert!(invocation(
            "shield_plan",
            json!({"input":"a.apk","output":"b.apk","exclude_abis":["mips","mips"]}),
            &policy
        )
        .is_err());
    }
}

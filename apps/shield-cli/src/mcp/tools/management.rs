//! 共享管理的参数与固定命令映射；不包含存储、默认项解析或密码规则。
use super::super::protocol::Management;
use super::*;
use shield_app::config::Setting;
#[cfg(test)]
#[path = "management_tests.rs"]
mod tests;

pub(in crate::mcp) fn management_tool(name: &str) -> Option<Management> {
    match name {
        "shield_settings" => Some(Management::Settings),
        "shield_update_settings" => Some(Management::UpdateSettings),
        "shield_update_certificate" => Some(Management::UpdateCertificate),
        "shield_set_default_certificate" => Some(Management::SetDefault),
        _ => super::certificates::kind(name),
    }
}
pub(super) fn list(execute: bool, shared: bool) -> Vec<Tool> {
    if !shared {
        return vec![];
    }
    let mut tools = vec![tool(
        "shield_settings",
        "查询共享加固业务偏好；不返回路径、界面、统计或范围外证书编号，不创建配置。",
        json!({}),
        &[],
    )];
    if !execute {
        return tools;
    }
    let id = json!({"type":"string","minLength":1,"maxLength":128});
    tools.push(tool("shield_update_settings","保存明确的共享模式、策略、自动签名意图及专用证书选择；省略保持，两个可空字段的 null 恢复继承。不加固、不改活动任务、不重复确认。",json!({
        "runtime_mode":{"type":"string","enum":["standard","android_api19"]},"environment_policy":{"type":"string","enum":["compatible","strict"]},
        "sign_after_protect":{"type":["boolean","null"]},"certificate_id":{"type":["string","null"],"minLength":1,"maxLength":128}}),&[]));
    tools.push(tool("shield_update_certificate","修改允许范围内证书的显示名称、备注及签名偏好，不改文件、Alias、类型或密码。未指定字段保持；回执不回显自由文本。",json!({
        "certificate_id":id,"name":{"type":"string","maxLength":256},"note":{"type":"string","maxLength":4096},
        "v1":{"type":"boolean"},"v2":{"type":"boolean"},"v3":{"type":"boolean"},"v4":{"type":"boolean"},"auto_sign":{"type":"boolean"}}),&["certificate_id"]));
    tools.push(tool("shield_set_default_certificate","将允许范围内的明确证书编号设为全局默认；不会清空默认或改变加固页专用选择，不要求额外计划或确认。",json!({"certificate_id":id}),&["certificate_id"]));
    for tool in tools.iter_mut().skip(1) {
        tool.annotations = Some(
            ToolAnnotations::new()
                .read_only(false)
                .destructive(true)
                .idempotent(false)
                .open_world(false),
        );
    }
    tools
}
#[derive(Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
struct Settings {
    runtime_mode: Setting<String>,
    environment_policy: Setting<String>,
    sign_after_protect: Setting<Option<bool>>,
    certificate_id: Setting<Option<String>>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Certificate {
    certificate_id: String,
    #[serde(default)]
    name: Setting<String>,
    #[serde(default)]
    note: Setting<String>,
    #[serde(default)]
    v1: Setting<bool>,
    #[serde(default)]
    v2: Setting<bool>,
    #[serde(default)]
    v3: Setting<bool>,
    #[serde(default)]
    v4: Setting<bool>,
    #[serde(default)]
    auto_sign: Setting<bool>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DefaultCertificate {
    certificate_id: String,
}
pub(in crate::mcp) fn management_call(
    name: &str,
    arguments: Value,
    policy: &Policy,
) -> Result<(Invocation, Management)> {
    if !policy.is_shared() {
        return Err("TOOL_NOT_AVAILABLE");
    }
    if !arguments.is_object() {
        return Err("ARGUMENT_INVALID");
    }
    let kind = management_tool(name).ok_or("TOOL_NOT_AVAILABLE")?;
    if kind.lifecycle() {
        return super::certificates::call(kind, arguments, policy).map(|call| (call, kind));
    }
    let mut command = policy.shared_arguments()?;
    match kind {
        Management::Settings => {
            if arguments.as_object().is_none_or(|map| !map.is_empty()) {
                return Err("ARGUMENT_INVALID");
            }
            command.extend(["config".into(), "show".into()]);
        }
        Management::UpdateSettings => {
            let args: Settings =
                serde_json::from_value(arguments).map_err(|_| "ARGUMENT_INVALID")?;
            command.extend(["config".into(), "set".into()]);
            let begin = command.len();
            for (key, value, allowed) in [
                (
                    "runtime-mode",
                    args.runtime_mode,
                    &["standard", "android_api19"][..],
                ),
                (
                    "environment-policy",
                    args.environment_policy,
                    &["compatible", "strict"][..],
                ),
            ] {
                if let Setting::Set(value) = value {
                    if !allowed.contains(&value.as_str()) {
                        return Err("ARGUMENT_INVALID");
                    }
                    command.push(format!("--{key}={value}").into());
                }
            }
            match args.sign_after_protect {
                Setting::Set(Some(value)) => {
                    command.push(format!("--sign-after-protect={value}").into())
                }
                Setting::Set(None) => command.push("--inherit-auto-sign".into()),
                _ => {}
            }
            match args.certificate_id {
                Setting::Set(Some(id)) => {
                    policy.certificate(&id)?;
                    command.push(format!("--certificate-id={id}").into());
                }
                Setting::Set(None) => command.push("--clear-certificate".into()),
                _ => {}
            }
            if command.len() == begin {
                return Err("PATCH_REQUIRED");
            }
        }
        Management::UpdateCertificate => {
            let args: Certificate =
                serde_json::from_value(arguments).map_err(|_| "ARGUMENT_INVALID")?;
            policy.certificate(&args.certificate_id)?;
            command.extend(["cert".into(), "edit".into()]);
            let begin = command.len();
            for (key, value, limit) in [("name", args.name, 256), ("note", args.note, 4096)] {
                if let Setting::Set(value) = value {
                    if value.len() > limit || value.contains('\0') {
                        return Err("ARGUMENT_INVALID");
                    }
                    command.push(format!("--{key}={value}").into());
                }
            }
            for (key, value) in [
                ("v1", args.v1),
                ("v2", args.v2),
                ("v3", args.v3),
                ("v4", args.v4),
                ("auto-sign", args.auto_sign),
            ] {
                if let Setting::Set(value) = value {
                    command.push(format!("--{key}={value}").into());
                }
            }
            if command.len() == begin {
                return Err("PATCH_REQUIRED");
            }
            command.extend(["--".into(), args.certificate_id.into()]);
        }
        Management::SetDefault => {
            let args: DefaultCertificate =
                serde_json::from_value(arguments).map_err(|_| "ARGUMENT_INVALID")?;
            policy.certificate(&args.certificate_id)?;
            command.extend([
                "cert".into(),
                "set-default".into(),
                "--".into(),
                args.certificate_id.into(),
            ]);
        }
        _ => unreachable!("生命周期命令由独立参数模块映射"),
    }
    Ok((
        Invocation {
            arguments: command,
            operation: kind.operation(),
            controlled: false,
            host_credentials: false,
        },
        kind,
    ))
}

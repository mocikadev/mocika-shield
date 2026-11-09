//! 证书生命周期的固定参数映射；不读取密码或数据库，不决定证书业务规则。
use super::super::protocol::Management;
use super::*;
#[cfg(test)]
#[path = "certificates_tests.rs"]
mod tests;

pub(super) fn kind(name: &str) -> Option<Management> {
    match name {
        "shield_verify_certificate" => Some(Management::Verify),
        "shield_import_certificate" => Some(Management::Import),
        "shield_create_certificate" => Some(Management::Create),
        "shield_remove_certificate" => Some(Management::Remove),
        _ => None,
    }
}
pub(super) fn list() -> Vec<Tool> {
    let id = json!({"type":"string","minLength":1,"maxLength":128});
    let mut tools = vec![
        tool(
            "shield_verify_certificate",
            "重新校验允许范围内的已保存证书并保存校验结果；使用本机已保存密码，不接受新口令。",
            json!({"certificate_id":id}),
            &["certificate_id"],
        ),
        tool(
            "shield_remove_certificate",
            "删除明确编号的证书记录，始终保留文件；不会自动选择替代证书，无第二次确认。",
            json!({"certificate_id":id}),
            &["certificate_id"],
        ),
    ];
    for (name, description, create) in [
        ("shield_import_certificate","导入输入根内相对路径的证书，保留原文件；仅使用宿主预置密码，单 Alias 可自动选择，成功后编号在当前会话直接可用。",false),
        ("shield_create_certificate","在共享托管目录创建证书，同名不覆盖；仅使用宿主预置密码，成功后编号在当前会话直接可用，不索要对话密码。",true),
    ] {
        let mut props = json!({
            "name":{"type":"string","minLength":1,"maxLength":256},
            "alias":{"type":"string","maxLength":128},
            "note":{"type":"string","maxLength":4096},
            "keystore_type":{"type":"string","enum":["jks","pkcs12"],"default":"jks"},
            "set_default":{"type":"boolean","default":false},
            "v1":{"type":"boolean"},"v2":{"type":"boolean"},"v3":{"type":"boolean"},"v4":{"type":"boolean"},"auto_sign":{"type":"boolean"}
        });
        let required = if create {
            props["file_name"] = json!({"type":"string","minLength":1,"maxLength":128});
            props["dname"] = json!({"type":"string","minLength":1,"maxLength":1024});
            props["validity_days"] = json!({"type":"integer","minimum":1,"maximum":36500,"default":36500});
            props["key_size"] = json!({"type":"integer","enum":[2048,4096],"default":2048});
            vec!["name","file_name","alias","dname"]
        } else {
            props["keystore"] = json!({"type":"string","minLength":1,"maxLength":2048,"description":"输入根内的普通文件相对路径，不接受链接或父级跳转"});
            vec!["name","keystore"]
        };
        tools.push(tool(name, description, props, &required));
    }
    for tool in &mut tools {
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

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Reference {
    certificate_id: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Material {
    name: String,
    keystore: Option<String>,
    file_name: Option<String>,
    alias: Option<String>,
    dname: Option<String>,
    keystore_type: Option<String>,
    note: Option<String>,
    validity_days: Option<u32>,
    key_size: Option<u32>,
    set_default: Option<bool>,
    v1: Option<bool>,
    v2: Option<bool>,
    v3: Option<bool>,
    v4: Option<bool>,
    auto_sign: Option<bool>,
}
fn text(value: &str, limit: usize, empty: bool) -> Result<()> {
    if value.len() > limit
        || (!empty && value.trim().is_empty())
        || value.chars().any(char::is_control)
    {
        Err("ARGUMENT_INVALID")
    } else {
        Ok(())
    }
}
pub(super) fn call(kind: Management, args: Value, policy: &Policy) -> Result<Invocation> {
    let map = args.as_object().ok_or("ARGUMENT_INVALID")?;
    if map.values().any(Value::is_null) {
        return Err("ARGUMENT_INVALID");
    }
    let mut command = policy.shared_arguments()?;
    command.push("cert".into());
    if matches!(kind, Management::Verify | Management::Remove) {
        let args: Reference = serde_json::from_value(args).map_err(|_| "ARGUMENT_INVALID")?;
        policy.certificate(&args.certificate_id)?;
        command.push(
            if matches!(kind, Management::Verify) {
                "verify"
            } else {
                "remove"
            }
            .into(),
        );
        if matches!(kind, Management::Remove) {
            command.push("--yes".into());
        }
        command.extend(["--".into(), args.certificate_id.into()]);
    } else {
        policy.can_add_certificate()?;
        let args: Material = serde_json::from_value(args).map_err(|_| "ARGUMENT_INVALID")?;
        let create = matches!(kind, Management::Create);
        if create && args.keystore.is_some()
            || !create
                && (args.file_name.is_some()
                    || args.dname.is_some()
                    || args.validity_days.is_some()
                    || args.key_size.is_some())
        {
            return Err("ARGUMENT_INVALID");
        }
        command.push(if create { "create" } else { "import" }.into());
        text(&args.name, 256, false)?;
        command.push(format!("--name={}", args.name).into());
        let format = args.keystore_type.as_deref().unwrap_or("jks");
        if !["jks", "pkcs12"].contains(&format) {
            return Err("ARGUMENT_INVALID");
        }
        command.push(format!("--keystore-type={format}").into());
        if create {
            let file = args.file_name.as_deref().ok_or("ARGUMENT_INVALID")?;
            text(file, 128, false)?;
            if file.contains(['/', '\\']) || [".", ".."].contains(&file) {
                return Err("ARGUMENT_INVALID");
            }
            command.push(format!("--file-name={file}").into());
            let dname = args.dname.as_deref().ok_or("ARGUMENT_INVALID")?;
            text(dname, 1024, false)?;
            command.push(format!("--dname={dname}").into());
            let days = args.validity_days.unwrap_or(36500);
            let size = args.key_size.unwrap_or(2048);
            if !(1..=36500).contains(&days) || ![2048, 4096].contains(&size) {
                return Err("ARGUMENT_INVALID");
            }
            command.extend([
                format!("--validity-days={days}").into(),
                format!("--key-size={size}").into(),
            ]);
        } else {
            let source = policy.input(args.keystore.as_deref().ok_or("ARGUMENT_INVALID")?)?;
            // 共享证书服务会去掉路径两端空白；不能让校验和最终保存指向另一份材料。
            if source.to_str().is_none_or(|path| path.trim() != path) {
                return Err("ARGUMENT_INVALID");
            }
            command.extend(["--keystore".into(), source.into_os_string()]);
        }
        let alias = args.alias.as_deref().unwrap_or("");
        text(alias, 128, !create)?;
        command.push(format!("--alias={alias}").into());
        if let Some(note) = args.note {
            text(&note, 4096, true)?;
            command.push(format!("--note={note}").into());
        }
        if args.set_default == Some(true) {
            command.push("--set-default".into());
        }
        for (key, value) in [
            ("v1", args.v1),
            ("v2", args.v2),
            ("v3", args.v3),
            ("v4", args.v4),
            ("auto-sign", args.auto_sign),
        ] {
            if let Some(value) = value {
                command.push(format!("--{key}={value}").into());
            }
        }
    }
    Ok(Invocation {
        arguments: command,
        operation: "cert",
        controlled: false,
        host_credentials: kind.adds_certificate(),
    })
}

//! 管理命令的有限响应，不把 CLI 的资料全文交给模型。
use super::*;
#[cfg(test)]
#[path = "management_tests.rs"]
mod tests;

#[derive(Clone, Copy)]
pub(in crate::mcp) enum Management {
    Settings,
    UpdateSettings,
    UpdateCertificate,
    SetDefault,
    Verify,
    Import,
    Create,
    Remove,
}
impl Management {
    pub fn operation(self) -> &'static str {
        match self {
            Self::Settings | Self::UpdateSettings => "config",
            _ => "cert",
        }
    }
    pub fn writes(self) -> bool {
        !matches!(self, Self::Settings)
    }
    pub fn adds_certificate(self) -> bool {
        matches!(self, Self::Import | Self::Create)
    }
    pub fn lifecycle(self) -> bool {
        matches!(
            self,
            Self::Verify | Self::Remove | Self::Import | Self::Create
        )
    }
}
fn identifier(value: &Value) -> Result<&str> {
    let value = value.as_str().ok_or("CLI_PROTOCOL_INVALID")?;
    if value.is_empty()
        || value.len() > 128
        || !value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
    {
        return Err("CLI_PROTOCOL_INVALID");
    }
    Ok(value)
}
fn choice(value: &Value, allowed: &[&str]) -> Result<Value> {
    if !value.as_str().is_some_and(|value| allowed.contains(&value)) {
        return Err("CLI_PROTOCOL_INVALID");
    }
    Ok(value.clone())
}
pub(in crate::mcp) fn decode_management(
    bytes: &[u8],
    kind: Management,
    exit: i32,
) -> Result<Value> {
    let events = events(bytes, kind.operation())?;
    if events.len() != 1 {
        return Err("CLI_PROTOCOL_INVALID");
    }
    let event = &events[0];
    if event["type"] == "error" && exit == 1 {
        let state = event["change_state"]
            .as_str()
            .ok_or("CLI_PROTOCOL_INVALID")?;
        if state == "confirmed" && matches!(kind, Management::Verify) {
            let value = &event["data"]["certificate"];
            if value["verification_status"] != "failed" {
                return Err("CLI_PROTOCOL_INVALID");
            }
            return Ok(
                json!({"schema_version":1,"operation":"cert","status":"blocked","code":code(&event["code"])?,"change_state":"confirmed","retryable":false,
                "data":{"certificate":{"id":identifier(&value["id"])?,"verification_status":"failed"}}}),
            );
        }
        if !["not_attempted", "unconfirmed"].contains(&state) {
            return Err("CLI_PROTOCOL_INVALID");
        }
        return Ok(
            json!({"schema_version":1,"operation":kind.operation(),"status":if kind.writes() && state=="unconfirmed" {"unknown"}else{"blocked"},"code":code(&event["code"])?,"change_state":state,"retryable":false}),
        );
    }
    if event["type"] != "result" || event["status"] != "ready" || exit != 0 {
        return Err("CLI_PROTOCOL_INVALID");
    }
    let data = &event["data"];
    let safe = match kind {
        Management::Settings | Management::UpdateSettings => {
            let value = &data["protect_defaults"];
            if value.get("sign_after_protect").is_none() || value.get("certificate_id").is_none() {
                return Err("CLI_PROTOCOL_INVALID");
            }
            if !value["sign_after_protect"].is_null() && !value["sign_after_protect"].is_boolean() {
                return Err("CLI_PROTOCOL_INVALID");
            }
            let certificate = if value["certificate_id"].is_null() {
                Value::Null
            } else {
                json!(identifier(&value["certificate_id"])?)
            };
            json!({"protect_defaults":{"runtime_mode":choice(&value["runtime_mode"],&["standard","android_api19"])?,
                "environment_policy":choice(&value["environment_policy"],&["compatible","strict"])?,
                "sign_after_protect":value["sign_after_protect"],"certificate_id":certificate}})
        }
        Management::UpdateCertificate | Management::Import | Management::Create => {
            let value = &data["certificate"];
            let versions = ["sign_v1", "sign_v2", "sign_v3", "sign_v4"]
                .iter()
                .map(|key| value[key].as_bool().ok_or("CLI_PROTOCOL_INVALID"))
                .collect::<Result<Vec<_>>>()?;
            json!({"certificate":{"id":identifier(&value["id"])?,"is_default":value["is_default"].as_bool().ok_or("CLI_PROTOCOL_INVALID")?,
                "signing_versions":versions,"auto_sign_enabled":value["auto_sign_enabled"].as_bool().ok_or("CLI_PROTOCOL_INVALID")?}})
        }
        Management::SetDefault => {
            json!({"default_certificate_id":identifier(&data["default_certificate_id"])?})
        }
        Management::Verify => {
            let value = &data["certificate"];
            if value["last_verify_status"] != "success" {
                return Err("CLI_PROTOCOL_INVALID");
            }
            json!({"certificate":{"id":identifier(&value["id"])?,"verification_status":"success"}})
        }
        Management::Remove => {
            if data["keystore_file"] != "retained" {
                return Err("CLI_PROTOCOL_INVALID");
            }
            json!({"removed_id":identifier(&data["removed_id"])?,"keystore_file":"retained"})
        }
    };
    Ok(
        json!({"schema_version":1,"operation":kind.operation(),"status":"ready","code":"OK","change_state":if kind.writes(){"confirmed"}else{"not_attempted"},"data":safe,"retryable":false}),
    )
}

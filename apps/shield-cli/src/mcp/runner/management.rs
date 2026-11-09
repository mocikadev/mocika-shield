//! 共享管理调用：复用桥接，区分已确认与未知修改；不复制存储或建立管理计划。
use super::super::protocol::Management;
use super::*;
#[cfg(test)]
#[path = "management_tests.rs"]
mod tests;

struct WriteAttempt {
    session: session::Shared,
    finished: bool,
}
impl Drop for WriteAttempt {
    fn drop(&mut self) {
        if !self.finished {
            session::lock(&self.session).block_execution();
        }
    }
}
pub(in crate::mcp) fn management_failure(
    code: &'static str,
    unknown: bool,
    operation: &str,
) -> Value {
    let status = if unknown {
        "unknown"
    } else {
        match code {
            "TASK_CANCELLED" => "cancelled",
            "TASK_TIMEOUT" => "timed_out",
            _ => "blocked",
        }
    };
    json!({"schema_version":1,"operation":operation,"status":status,"code":code,
        "change_state":if unknown{"unconfirmed"}else{"not_attempted"},"retryable":false})
}
impl Runner {
    pub(super) async fn manage(&self, name: &str, arguments: Value) -> Value {
        let kind = tools::management_tool(name).expect("管理工具已匹配");
        let prepared = self.check().and_then(|_| {
            if kind.writes() && session::lock(&self.session).execution_blocked() {
                return Err("SESSION_RECOVERY_REQUIRED");
            }
            tools::management_call(name, arguments.clone(), &self.policy)
        });
        let (call, kind) = match prepared {
            Ok(call) => call,
            Err(code) => {
                let mut value = management_failure(code, false, kind.operation());
                value["session_execution_blocked"] =
                    json!(session::lock(&self.session).execution_blocked());
                return value;
            }
        };
        let mut attempt = kind.writes().then(|| WriteAttempt {
            session: self.session.clone(),
            finished: false,
        });
        let decoded = match self.outcome(call, None, Some(self.deadline)).await {
            Ok(outcome) => protocol::decode_management(&outcome.bytes, kind, outcome.exit),
            Err(code) => Err(code),
        };
        let mut result = match decoded.and_then(|value| {
            self.check()?;
            if value["status"] == "ready" {
                self.management_postcheck(kind, &arguments, &value)?;
            }
            Ok(value)
        }) {
            Ok(value) => value,
            // 调用过桥接后，取消／超时也可能发生在 SQLite 或配置提交之后。
            Err(code) => management_failure(code, kind.writes(), kind.operation()),
        };
        if kind.writes() && result["change_state"] == "unconfirmed" {
            session::lock(&self.session).block_execution();
        }
        if result["status"] == "ready" {
            redact_settings(&mut result, &self.policy);
            if kind.writes() {
                result["updated_fields"] = if matches!(kind, Management::SetDefault) {
                    json!(["default_certificate_id"])
                } else {
                    json!(arguments
                        .as_object()
                        .unwrap()
                        .keys()
                        .filter(|key| *key != "certificate_id"
                            || matches!(kind, Management::UpdateSettings))
                        .cloned()
                        .collect::<Vec<_>>())
                };
            }
        }
        if let Some(attempt) = &mut attempt {
            attempt.finished = true;
        }
        result["session_execution_blocked"] =
            json!(session::lock(&self.session).execution_blocked());
        result
    }
    fn management_postcheck(&self, kind: Management, args: &Value, value: &Value) -> Result<()> {
        match kind {
            Management::Settings => {}
            Management::UpdateSettings => {
                for key in [
                    "runtime_mode",
                    "environment_policy",
                    "sign_after_protect",
                    "certificate_id",
                ] {
                    if let Some(expected) = args.get(key) {
                        if value["data"]["protect_defaults"][key] != *expected {
                            return Err("CLI_PROTOCOL_INVALID");
                        }
                    }
                }
            }
            Management::UpdateCertificate => {
                if value["data"]["certificate"]["id"] != args["certificate_id"] {
                    return Err("CLI_PROTOCOL_INVALID");
                }
                for (index, key) in ["v1", "v2", "v3", "v4"].iter().enumerate() {
                    if let Some(expected) = args.get(key) {
                        if value["data"]["certificate"]["signing_versions"][index] != *expected {
                            return Err("CLI_PROTOCOL_INVALID");
                        }
                    }
                }
                if let Some(expected) = args.get("auto_sign") {
                    if value["data"]["certificate"]["auto_sign_enabled"] != *expected {
                        return Err("CLI_PROTOCOL_INVALID");
                    }
                }
            }
            Management::SetDefault => {
                if value["data"]["default_certificate_id"] != args["certificate_id"] {
                    return Err("CLI_PROTOCOL_INVALID");
                }
            }
        }
        Ok(())
    }
}
fn redact_settings(value: &mut Value, policy: &Policy) {
    let Some(settings) = value
        .get_mut("data")
        .and_then(|data| data.get_mut("protect_defaults"))
    else {
        return;
    };
    let scope = match settings["certificate_id"].as_str() {
        None => "inherit_default",
        Some(id) if policy.certificate(id).is_ok() => "allowed",
        Some(_) => {
            settings["certificate_id"] = Value::Null;
            "outside_scope"
        }
    };
    settings["certificate_scope"] = json!(scope);
}

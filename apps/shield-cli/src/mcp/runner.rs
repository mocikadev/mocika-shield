//! 单任务会话编排；不复制核心规则，CLI 父端仍是唯一产物提交者。
use super::{
    bridge,
    policy::{Policy, Result},
    protocol::{self, Progress},
    session, tools,
};
use serde_json::{json, Value};
use std::sync::Arc;
use tokio::sync::mpsc::Sender;
use tokio_util::sync::CancellationToken;

pub(super) struct Runner {
    pub policy: Arc<Policy>,
    pub session: session::Shared,
    pub cancelled: CancellationToken,
    pub shutdown: CancellationToken,
    pub progress: Sender<Progress>,
}
impl Runner {
    pub async fn run(self, name: &str, arguments: Value) -> Value {
        if self.cancelled.is_cancelled() || self.shutdown.is_cancelled() {
            return failure("TASK_CANCELLED");
        }
        if name == "shield_protect" {
            return self.protect(arguments).await;
        }
        self.read(name, arguments).await.unwrap_or_else(failure)
    }
    fn check(&self) -> Result<()> {
        if let Err(code) = self.policy.check() {
            session::lock(&self.session).invalidate_plans();
            return Err(code);
        }
        Ok(())
    }
    async fn cli(&self, call: bridge::Invocation, id: Option<&str>) -> Result<Value> {
        let operation = call.operation;
        let controlled = call.controlled;
        let outcome = match bridge::run(
            &self.policy,
            call,
            self.cancelled.clone(),
            self.shutdown.clone(),
            |update| {
                if let Some(id) = id {
                    session::lock(&self.session).progress(id, update.stage);
                }
                if self.progress.try_send(update).is_err() {
                    self.cancelled.cancel();
                }
            },
        )
        .await
        {
            Ok(outcome) => outcome,
            Err(code) => {
                // 受控桥接仅在启动子进程前返回此取消码；它不是清场未知。
                if controlled && code != "TASK_CANCELLED"
                    || matches!(code, "STOP_UNCONFIRMED" | "CLI_EXIT_UNKNOWN")
                {
                    session::lock(&self.session).block_execution();
                }
                return Err(code);
            }
        };
        let mut value = match protocol::decode(&outcome.bytes, operation, outcome.exit) {
            Ok(value) => value,
            Err(code) => {
                if controlled {
                    session::lock(&self.session).block_execution();
                }
                return Err(code);
            }
        };
        if controlled && !value["plan"].is_null() {
            let valid = if self.policy.is_shared() {
                value["plan"]["schema_version"] == 3
                    && value["plan"]["shared_binding"]
                        .as_str()
                        .and_then(|binding| {
                            crate::automation::execution::shared_binding_certificate(binding).ok()
                        })
                        .is_some_and(|id| self.policy.certificate(&id).is_ok())
            } else {
                value["plan"]["schema_version"] == 2
            };
            if !valid {
                session::lock(&self.session).block_execution();
                return Err("CLI_PROTOCOL_INVALID");
            }
        }
        if controlled
            && (value["status"] == "unknown"
                || matches!(
                    value["cleanup_state"].as_str(),
                    Some("incomplete" | "retained" | "unconfirmed")
                ))
        {
            session::lock(&self.session).block_execution();
        }
        if let Some(cause) = outcome.stop_reason {
            // 已提交成功或未知提交的事实不能被晚取消覆盖。
            if ["ready", "warning", "cancelled"]
                .iter()
                .any(|status| value["status"] == *status)
            {
                value["status"] = json!(if cause == "TASK_TIMEOUT" {
                    "timed_out"
                } else {
                    "cancelled"
                });
                value["code"] = json!(cause);
                value.as_object_mut().unwrap().remove("plan");
            }
        }
        Ok(value)
    }
    async fn read(&self, name: &str, arguments: Value) -> Result<Value> {
        self.check()?;
        if name == "shield_plan" {
            session::lock(&self.session).can_plan()?;
        }
        let call = tools::invocation(name, arguments.clone(), &self.policy)?;
        let mut value = self.cli(call, None).await?;
        self.check()?;
        tools::invocation(name, arguments.clone(), &self.policy)?;
        if name == "shield_certificates" && value["status"] == "ready" {
            let selected = tools::certificate_reference(arguments.clone(), &self.policy)?;
            let rows = value["data"]["certificates"]
                .as_array_mut()
                .ok_or("CLI_PROTOCOL_INVALID")?;
            rows.retain(|row| {
                row["id"].as_str().is_some_and(|id| {
                    self.policy.certificate(id).is_ok()
                        && selected.as_ref().is_none_or(|selected| selected == id)
                })
            });
        }
        let binding = take_binding(&mut value)?;
        if name == "shield_plan" && matches!(value["status"].as_str(), Some("ready" | "warning")) {
            // 已请求取消的规划不能进入可执行缓存，即使 CLI 恰好完成检查。
            if self.cancelled.is_cancelled() || self.shutdown.is_cancelled() {
                return Err("TASK_CANCELLED");
            }
            let digest = value["plan"]["sha256"]
                .as_str()
                .ok_or("CLI_PROTOCOL_INVALID")?
                .to_owned();
            if self.policy.is_shared() {
                let selected = arguments["certificate_id"]
                    .as_str()
                    .ok_or("CERTIFICATE_REQUIRED")?;
                let bound = crate::automation::execution::shared_binding_certificate(
                    binding.as_deref().ok_or("CLI_PROTOCOL_INVALID")?,
                )?;
                if selected != bound {
                    session::lock(&self.session).block_execution();
                    return Err("CLI_PROTOCOL_INVALID");
                }
            }
            let id = session::lock(&self.session).insert_bound(arguments, digest, binding)?;
            value["plan_id"] = json!(id);
            value["expires_in_seconds"] = json!(600);
        }
        value["session_execution_blocked"] =
            json!(session::lock(&self.session).execution_blocked());
        Ok(value)
    }
    async fn protect(&self, arguments: Value) -> Value {
        let (id, digest) = match tools::protect_reference(arguments) {
            Ok(value) => value,
            Err(code) => return failure(code),
        };
        let (original, binding) = match session::lock(&self.session).begin(&id, &digest) {
            Ok(value) => value,
            Err(code) => return failure(code),
        };
        let mut attempt = session::Attempt::new(self.session.clone(), id.clone());
        let call = self.check().and_then(|_| {
            tools::protect(original.clone(), &self.policy, &digest, binding.as_deref())
        });
        let attempted_cli = call.is_ok();
        let mut result = match call {
            // 消费后、启动前的拒绝已知没有提交；同一计划仍不可重试。
            Err(code) => {
                let mut result = failure(code);
                result["cleanup_state"] = json!("not_needed");
                result
            }
            Ok(call) => match self.cli(call, Some(&id)).await {
                Ok(value) => value,
                Err("TASK_CANCELLED") => {
                    let mut result = failure("TASK_CANCELLED");
                    result["cleanup_state"] = json!("not_needed");
                    result
                }
                Err(code) => session::unknown(code),
            },
        };
        match take_binding(&mut result) {
            Ok(actual) if result["plan"].is_null() || actual == binding => {}
            _ => {
                result["status"] = json!("unknown");
                result["code"] = json!("CLI_PROTOCOL_INVALID");
            }
        }
        if result["plan"]["sha256"]
            .as_str()
            .is_some_and(|actual| !actual.eq_ignore_ascii_case(&digest))
        {
            result = session::unknown("CLI_PROTOCOL_INVALID");
        }
        // 执行后不能复用“输出不得存在”的规划检查，否则会把正常产物误报为冲突。
        let postcheck = if !attempted_cli {
            Ok(())
        } else {
            self.check().and_then(|_| {
                let input = original["input"].as_str().ok_or("ARGUMENT_INVALID")?;
                self.policy.input(input)?;
                if result["output_state"] == "published" {
                    let output = original["output"].as_str().ok_or("ARGUMENT_INVALID")?;
                    let path = self.policy.published_output(output)?;
                    let size = std::fs::metadata(path).map_err(|_| "OUTPUT_CHANGED")?.len();
                    if result["artifact"]["bytes"] != size {
                        return Err("OUTPUT_CHANGED");
                    }
                }
                Ok(())
            })
        };
        if let Err(code) = postcheck {
            if result["output_state"] == "published" {
                // 保留已经收到的交付证据，但不再宣称整项任务成功；绝不删除产物。
                result["status"] = json!("unknown");
                result["code"] = json!(code);
            } else if result["output_state"] == "uncertain" {
                result["code"] = json!(code);
            } else {
                result = failure(code);
            }
        }
        result["plan_id"] = json!(id);
        attempt.finish(result.clone());
        result["session_execution_blocked"] =
            json!(session::lock(&self.session).execution_blocked());
        result
    }
}

fn take_binding(value: &mut Value) -> Result<Option<String>> {
    match value
        .get_mut("plan")
        .and_then(Value::as_object_mut)
        .and_then(|plan| plan.remove("shared_binding"))
    {
        None => Ok(None),
        Some(Value::String(binding)) => Ok(Some(binding)),
        _ => Err("CLI_PROTOCOL_INVALID"),
    }
}

pub(super) fn failure(code: &'static str) -> Value {
    let status = match code {
        "TASK_CANCELLED" => "cancelled",
        "TASK_TIMEOUT" => "timed_out",
        "STOP_UNCONFIRMED" | "CLI_EXIT_UNKNOWN" | "TASK_STATE_UNKNOWN" => "unknown",
        _ => "blocked",
    };
    let message = if code == "SESSION_RECOVERY_REQUIRED" {
        "上一任务的提交或清理未确认，已停止新的规划和执行；请人工检查本机进程与产物，禁止自动重试、重连或重启服务。"
    } else {
        "本次调用未取得可用结果；不会自动重试，请检查本机授权范围与工具状态。"
    };
    json!({"schema_version":1,"status":status,"code":code,"output_state":"not_published","cleanup_state":"unconfirmed",
        "message":message,"retryable":false})
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn 启动前取消不锁会话但异常回执会锁会话且不泄漏原文() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("输入");
        let output = dir.path().join("输出");
        std::fs::create_dir(&input).unwrap();
        std::fs::create_dir(&output).unwrap();
        std::fs::write(input.join("a.apk"), b"fixture").unwrap();
        let cli = dir.path().join("shield");
        let config = dir.path().join("shield-cli.toml");
        std::fs::write(&cli, "#!/bin/sh\nprintf 'fixture-private-raw-output\\n'\n").unwrap();
        std::fs::set_permissions(&cli, std::fs::Permissions::from_mode(0o700)).unwrap();
        std::fs::write(&config, b"fixture").unwrap();
        // 仅私有单元夹具可指定程序；正式入口始终绑定 current_exe，不新增运行时注入开关。
        let policy = Arc::new(Policy::new(&input, &output, Some(&config), &cli, 5).unwrap());
        let session = Arc::new(std::sync::Mutex::new(session::Session::default()));
        let id = session::lock(&session)
            .insert(json!({"input":"a.apk","output":"b.apk"}), "a".repeat(64))
            .unwrap();
        let (progress, _reader) = tokio::sync::mpsc::channel(16);
        let mut runner = Runner {
            policy,
            session: session.clone(),
            cancelled: CancellationToken::new(),
            shutdown: CancellationToken::new(),
            progress,
        };
        runner.cancelled.cancel();
        let cancelled = runner
            .protect(json!({"plan_id":id,"expect_plan_sha256":"a".repeat(64)}))
            .await;
        assert_eq!(cancelled["status"], "cancelled");
        assert_eq!(cancelled["cleanup_state"], "not_needed");
        assert_eq!(cancelled["session_execution_blocked"], false);
        assert!(session::lock(&session).can_plan().is_ok());
        let id = session::lock(&session)
            .insert(json!({"input":"a.apk","output":"b.apk"}), "a".repeat(64))
            .unwrap();
        runner.cancelled = CancellationToken::new();
        let result = runner
            .run(
                "shield_protect",
                json!({"plan_id":id,"expect_plan_sha256":"a".repeat(64)}),
            )
            .await;
        assert_eq!(result["status"], "unknown");
        assert_eq!(result["output_state"], "uncertain");
        assert!(!result.to_string().contains("fixture-private-raw-output"));
        assert_eq!(
            session::lock(&session).status(&id).unwrap()["state"],
            "finished"
        );
        assert!(session::lock(&session).begin(&id, &"a".repeat(64)).is_err());
        assert_eq!(
            session::lock(&session).can_plan(),
            Err("SESSION_RECOVERY_REQUIRED")
        );
        assert!(!output.join("b.apk").exists());
    }
}

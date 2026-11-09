//! 公共终态白名单；保留交付与收尾事实，不序列化路径、配置或工具日志。
use super::*;

#[derive(Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(super) enum Status {
    Ready,
    Warning,
    Blocked,
    Succeeded,
    Failed,
    Cancelled,
    TimedOut,
    Unknown,
}
#[derive(Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum Output {
    NotPublished,
    Published,
    Uncertain,
}
#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
enum Cleanup {
    NotNeeded,
    Complete,
    Incomplete,
    Retained,
    Unconfirmed,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Artifact {
    bytes: u64,
    sha256: String,
}
#[derive(Serialize)]
struct Plan {
    schema_version: u32,
    sha256: String,
    private_key_access_checked: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    shared_binding: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Raw {
    #[serde(rename = "type")]
    kind: String,
    status: Status,
    output_state: Output,
    cleanup_complete: bool,
    retryable: bool,
    elapsed_ms: u128,
    plan_digest: String,
    code: Option<String>,
    artifact: Option<Artifact>,
    directory_synced: Option<bool>,
    shared_binding: Option<String>,
    signing_warning: Option<bool>,
    task_kind: Option<String>,
}

#[derive(Serialize)]
pub(super) struct Terminal {
    status: Status,
    code: String,
    output_state: Output,
    cleanup_state: Cleanup,
    directory_synced: Option<bool>,
    retryable: bool,
    pub elapsed_ms: u128,
    #[serde(skip_serializing_if = "Option::is_none")]
    plan: Option<Plan>,
    #[serde(skip_serializing_if = "Option::is_none")]
    artifact: Option<Artifact>,
    #[serde(skip_serializing_if = "Option::is_none")]
    cancel_source: Option<&'static str>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    warnings: Vec<&'static str>,
}
impl Terminal {
    pub fn before_start(code: &str) -> Self {
        Self {
            status: Status::Blocked,
            code: code.to_owned(),
            output_state: Output::NotPublished,
            cleanup_state: Cleanup::NotNeeded,
            directory_synced: None,
            retryable: false,
            elapsed_ms: 0,
            plan: None,
            artifact: None,
            cancel_source: None,
            warnings: vec![],
        }
    }

    fn error(err: CheckError) -> Self {
        let mut result = Self::before_start(err.code);
        result.cleanup_state = match err.cleanup_complete {
            Some(true) => Cleanup::Complete,
            Some(false) => Cleanup::Incomplete,
            None => Cleanup::Unconfirmed,
        };
        result.status = match err.code {
            "TASK_CANCELLED" => Status::Cancelled,
            "TASK_TIMEOUT" => Status::TimedOut,
            "STOP_UNCONFIRMED" | "COMMIT_UNCONFIRMED" | "TASK_STATE_UNKNOWN" => Status::Unknown,
            "PLAN_BLOCKED"
            | "PLAN_MISMATCH"
            | "SIGNING_V4_NOT_SUPPORTED"
            | "OUTPUT_EXISTS"
            | "SHARED_BINDING_INVALID" => Status::Blocked,
            _ => Status::Failed,
        };
        match err.code {
            "COMMIT_UNCONFIRMED" | "TASK_STATE_UNKNOWN" => {
                result.output_state = Output::Uncertain;
                result.cleanup_state = Cleanup::Retained;
            }
            "STOP_UNCONFIRMED" => result.cleanup_state = Cleanup::Retained,
            _ => {}
        }
        result
    }

    #[cfg(test)]
    pub fn from_run(run: Result<Value, CheckError>, plan_only: bool) -> Self {
        Self::from_operation(run, plan_only, false)
    }
    pub fn from_operation(run: Result<Value, CheckError>, plan_only: bool, signing: bool) -> Self {
        let raw = match run {
            Err(err) => return Self::error(err),
            Ok(value) => match serde_json::from_value::<Raw>(value) {
                Ok(raw) => raw,
                Err(_) => return Self::error(error("TASK_STATE_UNKNOWN")),
            },
        };
        let artifact_valid = raw
            .artifact
            .as_ref()
            .is_none_or(|artifact| artifact.bytes > 0 && parse_hex::<32>(&artifact.sha256).is_ok());
        let invalid = raw.kind != "result"
            || !matches!(
                (signing, raw.task_kind.as_deref()),
                (true, Some("sign")) | (false, None)
            )
            || (signing && raw.shared_binding.is_some())
            || (!signing && raw.signing_warning.is_some())
            || raw.shared_binding.as_deref().is_some_and(|binding| {
                crate::automation::shared_plan::Binding::decode(binding).is_err()
            })
            || raw.retryable
            || parse_hex::<32>(&raw.plan_digest).is_err()
            || !artifact_valid
            || raw.code.as_ref().is_some_and(|code| {
                code.is_empty()
                    || code.len() > 64
                    || !code
                        .bytes()
                        .all(|ch| ch.is_ascii_uppercase() || ch.is_ascii_digit() || ch == b'_')
            })
            || (!plan_only && matches!(raw.status, Status::Ready | Status::Warning))
            || (plan_only && (raw.output_state != Output::NotPublished || raw.artifact.is_some()))
            || (raw.status == Status::Succeeded
                && (raw.output_state != Output::Published
                    || raw.artifact.is_none()
                    || raw.directory_synced.is_none()))
            || (raw.output_state == Output::Published
                && (!matches!(raw.status, Status::Succeeded | Status::Unknown)
                    || raw.artifact.is_none()
                    || raw.directory_synced.is_none()))
            || (raw.output_state != Output::Published
                && (raw.artifact.is_some() || raw.directory_synced.is_some()))
            || (raw.output_state == Output::Uncertain
                && (raw.status != Status::Unknown || raw.cleanup_complete))
            || (matches!(raw.status, Status::Failed | Status::Blocked)
                && raw.code.as_deref().is_none_or(|code| code == "OK"));
        if invalid {
            return Self::error(error("TASK_STATE_UNKNOWN"));
        }
        let cleanup_state = if raw.cleanup_complete {
            Cleanup::Complete
        } else if raw.status == Status::Unknown || raw.output_state == Output::Uncertain {
            Cleanup::Retained
        } else {
            Cleanup::Incomplete
        };
        let code = raw.code.unwrap_or_else(|| {
            match raw.status {
                Status::Cancelled => "TASK_CANCELLED",
                Status::TimedOut => "TASK_TIMEOUT",
                Status::Unknown => "TASK_STATE_UNKNOWN",
                _ => "OK",
            }
            .to_owned()
        });
        let warnings = if raw.signing_warning == Some(true)
            || (signing && matches!(raw.status, Status::Warning))
        {
            vec!["SIGNING_CERTIFICATE_CHANGED"]
        } else {
            vec![]
        };
        Self {
            status: raw.status,
            code,
            output_state: raw.output_state,
            cleanup_state,
            directory_synced: raw.directory_synced,
            retryable: false,
            elapsed_ms: raw.elapsed_ms,
            plan: Some(Plan {
                schema_version: if signing {
                    4
                } else if raw.shared_binding.is_some() {
                    3
                } else {
                    2
                },
                sha256: raw.plan_digest,
                private_key_access_checked: !plan_only && raw.artifact.is_some(),
                shared_binding: raw.shared_binding,
            }),
            artifact: raw.artifact,
            cancel_source: None,
            warnings,
        }
    }

    pub fn apply_cancel(&mut self, cause: Cause) {
        self.cancel_source = match cause {
            Cause::None => None,
            Cause::Input => Some("stdin"),
            Cause::Closed => Some("stdin_closed"),
            Cause::Invalid => Some("invalid_control"),
            Cause::Signal => Some("signal"),
            Cause::Output => Some("output_closed"),
        };
        if matches!(self.status, Status::Ready | Status::Warning) && cause != Cause::None {
            self.status = Status::Cancelled;
        }
        if self.status == Status::Cancelled {
            match cause {
                Cause::Invalid => {
                    self.status = Status::Failed;
                    self.code = "CONTROL_INVALID".to_owned();
                }
                Cause::Closed | Cause::Output => self.code = "CLIENT_DISCONNECTED".to_owned(),
                _ => self.code = "TASK_CANCELLED".to_owned(),
            }
        } else if self.status == Status::TimedOut {
            self.code = "TASK_TIMEOUT".to_owned();
        } else if self.status == Status::Unknown && self.code == "OK" {
            self.code = "TASK_STATE_UNKNOWN".to_owned();
        }
    }

    pub fn exit(&self) -> i32 {
        match self.status {
            Status::Ready | Status::Warning | Status::Succeeded => 0,
            Status::Failed | Status::Blocked => 1,
            Status::Unknown => 3,
            Status::TimedOut => 124,
            Status::Cancelled => 130,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn 规划错误只在持有实际清场证据时才确认清理() {
        for (proof, expected) in [
            (None, "unconfirmed"),
            (Some(false), "incomplete"),
            (Some(true), "complete"),
        ] {
            let mut error = error("SIGNING_V4_NOT_SUPPORTED");
            error.cleanup_complete = proof;
            let value = serde_json::to_value(Terminal::error(error)).unwrap();
            assert_eq!(value["cleanup_state"], expected);
            assert_eq!(value["output_state"], "not_published");
        }
        let value = serde_json::to_value(Terminal::error(
            error("STOP_UNCONFIRMED").with_cleanup(true),
        ))
        .unwrap();
        assert_eq!(value["cleanup_state"], "retained");
        assert_eq!(value["status"], "unknown");
    }
    #[test]
    fn 签名结果有独立种类且不能被加固结果解析器接受() {
        let sign = serde_json::json!({"type":"result","task_kind":"sign","status":"ready","output_state":"not_published", "cleanup_complete":true,
            "retryable":false,"elapsed_ms":1,"plan_digest":"a".repeat(64)});
        assert_eq!(Terminal::from_run(Ok(sign.clone()), true).exit(), 3);
        let value =
            serde_json::to_value(Terminal::from_operation(Ok(sign.clone()), true, true)).unwrap();
        assert_eq!(value["plan"]["schema_version"], 4);
        let mut old = sign;
        old.as_object_mut().unwrap().remove("task_kind");
        assert_eq!(Terminal::from_operation(Ok(old), true, true).exit(), 3);
    }
    #[test]
    fn 矛盾终态和缺失失败码必须失败关闭() {
        let base = serde_json::json!({"type":"result","status":"failed","output_state":"not_published","cleanup_complete":true,
            "retryable":false,"elapsed_ms":1,"plan_digest":"a".repeat(64)});
        let mut cases = vec![base.clone()];
        let mut cancelled = base.clone();
        cancelled["status"] = serde_json::json!("cancelled");
        cancelled["output_state"] = serde_json::json!("published");
        cancelled["artifact"] = serde_json::json!({"bytes":1,"sha256":"b".repeat(64)});
        cancelled["directory_synced"] = serde_json::json!(true);
        cases.push(cancelled);
        let mut invalid_sync = base.clone();
        invalid_sync["code"] = serde_json::json!("TOOL_PROCESS_FAILED");
        invalid_sync["directory_synced"] = serde_json::json!(true);
        cases.push(invalid_sync);
        let mut uncertain = base;
        uncertain["status"] = serde_json::json!("unknown");
        uncertain["output_state"] = serde_json::json!("uncertain");
        cases.push(uncertain);
        for value in cases {
            let terminal = Terminal::from_run(Ok(value), false);
            assert_eq!(terminal.exit(), 3);
            assert_eq!(
                serde_json::to_value(terminal).unwrap()["code"],
                "TASK_STATE_UNKNOWN"
            );
        }
    }
    #[test]
    fn 未知交付不误报无输出且不能自动重试() {
        let terminal = Terminal::error(error("COMMIT_UNCONFIRMED"));
        let value = serde_json::to_value(&terminal).unwrap();
        assert_eq!(value["status"], "unknown");
        assert_eq!(value["output_state"], "uncertain");
        assert_eq!(value["retryable"], false);
        assert_eq!(terminal.exit(), 3);
    }
    #[test]
    fn 已发布事实不被晚到取消或清理失败抹掉() {
        let raw = serde_json::json!({"type":"result","status":"succeeded","output_state":"published","cleanup_complete":false,
            "retryable":false,"elapsed_ms":1,"plan_digest":"a".repeat(64),"directory_synced":false,
            "artifact":{"bytes":1,"sha256":"b".repeat(64)}});
        let mut terminal = Terminal::from_run(Ok(raw), false);
        terminal.apply_cancel(Cause::Signal);
        let value = serde_json::to_value(&terminal).unwrap();
        assert_eq!(value["status"], "succeeded");
        assert_eq!(value["output_state"], "published");
        assert_eq!(value["cleanup_state"], "incomplete");
        assert_eq!(value["directory_synced"], false);
        assert_eq!(terminal.exit(), 0);
    }
}

//! 单次签名调用编排：固定证书编号后直接复用 CLI，不新增待确认计划。
use super::*;

impl Runner {
    async fn signing_certificate(&self, args: &tools::Sign) -> Result<String> {
        if let Some(id) = &args.certificate_id {
            self.policy.certificate(id)?;
            return Ok(id.clone());
        }
        // 唯一允许项无需额外查询；证书实际存在性由受控 CLI 检查。
        if let Some(id) = self.policy.sole_certificate()? {
            return Ok(id.to_owned());
        }
        let call = tools::invocation("shield_certificates", json!({}), &self.policy)?;
        let listing = self.cli_until(call, None, Some(self.deadline)).await?;
        if listing["status"] != "ready" {
            return Err("CREDENTIAL_UNAVAILABLE");
        }
        let rows = listing["data"]["certificates"]
            .as_array()
            .ok_or("CLI_PROTOCOL_INVALID")?;
        let mut defaults = rows
            .iter()
            .filter(|row| row["is_default"] == true)
            .filter_map(|row| row["id"].as_str())
            .filter(|id| self.policy.certificate(id).is_ok());
        let selected = defaults.next().ok_or("CERTIFICATE_REQUIRED")?.to_owned();
        if defaults.next().is_some() {
            return Err("CLI_PROTOCOL_INVALID");
        }
        Ok(selected)
    }
    pub(super) async fn sign(&self, arguments: Value) -> Value {
        let prepared = self
            .check()
            .and_then(|_| tools::sign_arguments(arguments.clone(), &self.policy));
        let args = match prepared {
            Ok(args) => args,
            Err(code) => return failure(code),
        };
        let id = match session::lock(&self.session).start_sign() {
            Ok(id) => id,
            Err(code) => return failure(code),
        };
        let mut attempt = session::Attempt::new(self.session.clone(), id.clone());
        let call = match self.signing_certificate(&args).await {
            Ok(certificate) => self
                .check()
                .and_then(|_| tools::sign_call(&args, &self.policy, &certificate)),
            Err(code) => Err(code),
        };
        let attempted = call.is_ok();
        let mut result = match call {
            Err(code) => before_start(code),
            Ok(call) => match self.cli_until(call, Some(&id), Some(self.deadline)).await {
                Ok(value) => value,
                Err(code @ ("TASK_CANCELLED" | "TASK_TIMEOUT")) => before_start(code),
                Err(code) => session::unknown_for(code, "agent_sign"),
            },
        };
        self.postcheck(&arguments, &mut result, attempted);
        result["operation"] = json!("agent_sign");
        result["plan_id"] = json!(id);
        attempt.finish(result.clone());
        result["session_execution_blocked"] =
            json!(session::lock(&self.session).execution_blocked());
        result
    }
}
fn before_start(code: &'static str) -> Value {
    if matches!(code, "STOP_UNCONFIRMED" | "CLI_EXIT_UNKNOWN") {
        return session::unknown_for(code, "agent_sign");
    }
    let mut result = failure(code);
    result["cleanup_state"] = json!("not_needed");
    result
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::{fs, os::unix::fs::PermissionsExt, path::Path, sync::Mutex, time::Duration};
    fn policy(root: &Path, ids: Vec<String>) -> Arc<Policy> {
        fs::create_dir(root.join("输入")).unwrap();
        fs::create_dir(root.join("输出")).unwrap();
        fs::write(root.join("输入/a.apk"), b"fixture").unwrap();
        let cli = root.join("shield");
        fs::write(&cli, "#!/bin/sh\nprintf 'private-invalid-cli-output\\n'\n").unwrap();
        fs::set_permissions(&cli, fs::Permissions::from_mode(0o700)).unwrap();
        Arc::new(
            Policy::new(&root.join("输入"), &root.join("输出"), None, &cli, 30)
                .unwrap()
                .with_shared(Some(&root.join("共享资料")), ids)
                .unwrap(),
        )
    }
    fn runner(
        policy: Arc<Policy>,
        session: session::Shared,
        deadline: tokio::time::Instant,
    ) -> Runner {
        let (progress, _reader) = tokio::sync::mpsc::channel(16);
        Runner {
            policy,
            session,
            cancelled: CancellationToken::new(),
            shutdown: CancellationToken::new(),
            progress,
            deadline,
        }
    }
    #[tokio::test]
    async fn 默认查询也受原调用期限约束且超时前未启动写任务不锁会话() {
        let root = tempfile::tempdir().unwrap();
        let policy = policy(root.path(), vec!["one".into(), "two".into()]);
        let session = Arc::new(Mutex::new(session::Session::default()));
        let value = runner(
            policy,
            session.clone(),
            tokio::time::Instant::now() - Duration::from_secs(1),
        )
        .run("shield_sign", json!({"input":"a.apk","output":"b.apk"}))
        .await;
        assert_eq!(value["status"], "timed_out");
        assert_eq!(value["cleanup_state"], "not_needed");
        assert_eq!(value["session_execution_blocked"], false);
        assert!(session::lock(&session).can_plan().is_ok());
        assert!(!root.path().join("输出/b.apk").exists());
    }
    #[tokio::test]
    async fn 签名异常回执保持正确任务种类并锁住后续加固和签名() {
        let root = tempfile::tempdir().unwrap();
        let policy = policy(root.path(), vec!["one".into()]);
        let session = Arc::new(Mutex::new(session::Session::default()));
        let pending = session::lock(&session)
            .insert(json!({}), "a".repeat(64))
            .unwrap();
        let value = runner(
            policy.clone(),
            session.clone(),
            tokio::time::Instant::now() + Duration::from_secs(5),
        )
        .run("shield_sign", json!({"input":"a.apk","output":"b.apk"}))
        .await;
        assert_eq!(value["status"], "unknown");
        assert_eq!(value["operation"], "agent_sign");
        assert_eq!(value["session_execution_blocked"], true);
        assert!(!value.to_string().contains("private-invalid-cli-output"));
        assert_eq!(
            session::lock(&session).begin(&pending, &"a".repeat(64)),
            Err("SESSION_RECOVERY_REQUIRED")
        );
        let next = runner(
            policy,
            session,
            tokio::time::Instant::now() + Duration::from_secs(5),
        )
        .run("shield_sign", json!({"input":"a.apk","output":"c.apk"}))
        .await;
        assert_eq!(next["code"], "SESSION_RECOVERY_REQUIRED");
    }
}

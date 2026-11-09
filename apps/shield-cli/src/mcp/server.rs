//! SDK 接口和进度适配；会话规则与 CLI 监督在各自私有模块中。
use super::{
    policy::Policy,
    protocol::Progress,
    runner::{self, Runner},
    session, tools,
};
use rmcp::{model::*, service::RequestContext, ErrorData, RoleServer, ServerHandler};
use serde_json::Value;
use std::{borrow::Cow, sync::Arc, time::Duration};
use tokio::sync::Semaphore;
use tokio_util::sync::CancellationToken;

pub(super) struct Server {
    pub policy: Arc<Policy>,
    pub gate: Arc<Semaphore>,
    pub shutdown: CancellationToken,
    pub session: session::Shared,
    pub allow_execution: bool,
}
impl ServerHandler for Server {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new("mocika-shield", env!("CARGO_PKG_VERSION")))
            .with_protocol_version(ProtocolVersion::V_2025_11_25)
            .with_instructions("只处理宿主固定目录与配置。已开启执行时，加固使用本会话计划及匹配摘要，计划一次消费；共享签名和资料管理一次调用完成，无额外计划或重复确认。仅在用户明确要求时保存、创建、导入或删除记录，文件始终保留；校验也会保存状态。创建／导入只使用宿主预置密码，禁止索要对话密码；成功的新编号当前会话直接可用。修改未确认不得称回滚或自动重试。禁止任意命令、设备安装或发布。未显式允许执行时只有只读工具。")
    }
    fn supported_protocol_versions(&self) -> Cow<'static, [ProtocolVersion]> {
        Cow::Owned(vec![ProtocolVersion::V_2025_11_25])
    }
    async fn list_tools(
        &self,
        request: Option<PaginatedRequestParams>,
        _: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        if request.is_some_and(|request| request.cursor.is_some()) {
            return Err(ErrorData::invalid_params("不支持该分页位置", None));
        }
        Ok(ListToolsResult::with_all_items(tools::list(
            self.allow_execution,
            self.policy.is_shared(),
        )))
    }
    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        let management = tools::management_tool(&request.name);
        if request.input_responses.is_some() || request.request_state.is_some() {
            return Ok(error_for("EXTENSION_NOT_SUPPORTED", management).into());
        }
        let arguments = Value::Object(request.arguments.unwrap_or_default());
        if request.name == "shield_execution_status" && self.allow_execution {
            let result = tools::status_reference(arguments)
                .and_then(|id| session::lock(&self.session).status(&id));
            return Ok(match result {
                Ok(value) => CallToolResult::structured(value),
                Err(code) => error(code),
            }
            .into());
        }
        let available = matches!(
            request.name.as_ref(),
            "shield_doctor" | "shield_preflight" | "shield_plan"
        ) || self.allow_execution && request.name == "shield_protect"
            || self.allow_execution && self.policy.is_shared() && request.name == "shield_sign"
            || self.policy.is_shared() && request.name == "shield_certificates"
            || self.policy.is_shared()
                && tools::management_tool(&request.name)
                    .is_some_and(|kind| !kind.writes() || self.allow_execution);
        if !available {
            return Ok(error_for("TOOL_NOT_AVAILABLE", management).into());
        }
        let permit = match self.gate.clone().try_acquire_owned() {
            Ok(value) => value,
            Err(_) => return Ok(error_for("SERVER_BUSY", management).into()),
        };
        let (progress, mut updates) = tokio::sync::mpsc::channel::<Progress>(16);
        let cancelled = context.ct.clone();
        let runner = Runner {
            policy: self.policy.clone(),
            session: self.session.clone(),
            cancelled: cancelled.clone(),
            shutdown: self.shutdown.clone(),
            progress,
            deadline: tokio::time::Instant::now()
                + Duration::from_secs(self.policy.timeout_seconds),
        };
        let operation = if request.name == "shield_sign" {
            "agent_sign"
        } else {
            "agent_protect"
        };
        // 后台调用持有许可和收尾责任；SDK 取消原响应不能丢掉正在执行的任务。
        let mut work = tokio::spawn(async move {
            let _permit = permit;
            runner.run(&request.name, arguments).await
        });
        let mut token = context.meta.get_progress_token();
        let value = loop {
            tokio::select! {
                result = &mut work => break result.unwrap_or_else(|_| match management {
                    Some(kind)=>runner::management_failure("TASK_STATE_UNKNOWN",kind.writes(),kind.operation()),
                    None=>session::unknown_for("TASK_STATE_UNKNOWN", operation),
                }),
                Some(update) = updates.recv(), if token.is_some() && !cancelled.is_cancelled() => {
                    let mut notification = ProgressNotificationParam::new(token.clone().unwrap(),f64::from(update.index)+1.0).with_message(update.stage);
                    if let Some(id) = update.plan_id {
                        let mut meta = NotificationMetaObject::new();
                        meta.insert("mocika-shield/plan_id".into(), Value::String(id));
                        notification.meta = Some(meta);
                    }
                    if !matches!(tokio::time::timeout(Duration::from_secs(1),context.peer.notify_progress(notification)).await,Ok(Ok(()))) {
                        cancelled.cancel(); token=None;
                    }
                }
                _ = cancelled.cancelled(), if token.is_some() => { token=None; }
            }
        };
        let result = if matches!(
            value["status"].as_str(),
            Some("ready" | "warning" | "succeeded")
        ) {
            CallToolResult::structured(value)
        } else {
            CallToolResult::structured_error(value)
        };
        Ok(result.into())
    }
}
fn error(code: &'static str) -> CallToolResult {
    CallToolResult::structured_error(runner::failure(code))
}
fn error_for(code: &'static str, kind: Option<super::protocol::Management>) -> CallToolResult {
    match kind {
        Some(kind) => CallToolResult::structured_error(runner::management_failure(
            code,
            false,
            kind.operation(),
        )),
        None => error(code),
    }
}

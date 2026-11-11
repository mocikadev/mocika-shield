//! 自动化入口适配：默认只读；桌面开发特性复用共享资料与受控执行，不复制存储规则。
#[cfg(all(
    feature = "agent-executor",
    any(target_os = "macos", target_os = "linux", windows)
))]
mod adapter;
mod args;
mod doctor;
mod java_tools;
mod plan;
mod preflight;
mod resources;
mod snapshot;
// 源码的桌面执行特性显式开启；Unix 的既有默认单元测试继续保留。
#[cfg(any(
    all(unix, test),
    all(
        feature = "agent-executor",
        any(target_os = "macos", target_os = "linux", windows)
    )
))]
mod cancellation;
#[cfg(any(
    all(unix, test),
    all(
        feature = "agent-executor",
        any(target_os = "macos", target_os = "linux", windows)
    )
))]
mod event_output;
#[cfg(any(
    all(unix, test),
    all(
        feature = "agent-executor",
        any(target_os = "macos", target_os = "linux", windows)
    )
))]
mod executable;
#[cfg(all(
    feature = "agent-executor",
    any(target_os = "macos", target_os = "linux", windows)
))]
pub(crate) mod execution;
#[cfg(all(test, unix))]
mod executor_contract_tests;
#[cfg(any(
    all(unix, test),
    all(
        feature = "agent-executor",
        any(target_os = "macos", target_os = "linux", windows)
    )
))]
mod java_environment;
#[cfg(any(
    all(unix, test),
    all(
        feature = "agent-executor",
        any(target_os = "macos", target_os = "linux", windows)
    )
))]
pub(crate) mod native_group;
#[cfg(any(
    all(unix, test),
    all(
        feature = "agent-executor",
        any(target_os = "macos", target_os = "linux", windows)
    )
))]
mod private_json;
#[cfg(any(
    all(unix, test),
    all(
        feature = "agent-executor",
        any(target_os = "macos", target_os = "linux", windows)
    )
))]
mod publication;
#[cfg(any(
    all(unix, test),
    all(
        feature = "agent-executor",
        any(target_os = "macos", target_os = "linux", windows)
    )
))]
pub(crate) mod random_id;
#[cfg(all(
    feature = "agent-executor",
    any(target_os = "macos", target_os = "linux", windows)
))]
mod shared_plan;
#[cfg(all(
    feature = "agent-executor",
    any(target_os = "macos", target_os = "linux", windows)
))]
mod sign_plan;
#[cfg(any(
    all(unix, test),
    all(
        feature = "agent-executor",
        any(target_os = "macos", target_os = "linux", windows)
    )
))]
pub(crate) mod task_files;
#[cfg(any(
    test,
    all(
        feature = "agent-executor",
        any(target_os = "macos", target_os = "linux", windows)
    )
))]
mod task_lifecycle;
#[cfg(any(
    all(unix, test),
    all(
        feature = "agent-executor",
        any(target_os = "macos", target_os = "linux", windows)
    )
))]
mod task_materials;
#[cfg(all(
    feature = "mcp-server",
    any(target_os = "macos", target_os = "linux", windows)
))]
pub(crate) mod transport;
#[cfg(all(windows, feature = "agent-executor"))]
mod windows_job;
#[cfg(any(
    test,
    all(
        feature = "agent-executor",
        any(target_os = "macos", target_os = "linux", windows)
    )
))]
mod worker_protocol;

pub(crate) use args::{DoctorArgs, PlanProtectArgs, PreflightArgs};

use serde::Serialize;
use serde_json::{json, Value};
use shield_core::diagnostic::Diagnostic;
use std::fs::File;
use std::path::Path;

#[derive(Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum Status {
    Ready,
    Warning,
    Blocked,
}

struct CheckResult {
    status: Status,
    data: Value,
}

struct CheckError {
    code: &'static str,
    message: &'static str,
    #[cfg(all(
        feature = "agent-executor",
        any(target_os = "macos", target_os = "linux", windows)
    ))]
    cleanup_complete: Option<bool>,
}

impl CheckError {
    fn new(code: &'static str, message: &'static str) -> Self {
        Self {
            code,
            message,
            #[cfg(all(
                feature = "agent-executor",
                any(target_os = "macos", target_os = "linux", windows)
            ))]
            cleanup_complete: None,
        }
    }
    #[cfg(all(
        feature = "agent-executor",
        any(target_os = "macos", target_os = "linux", windows)
    ))]
    fn with_cleanup(mut self, complete: bool) -> Self {
        self.cleanup_complete = Some(complete);
        self
    }

    fn from_core(error: anyhow::Error) -> Self {
        // 只映射固定分类，不把路径、APK 元数据或工具输出送入模型上下文。
        Self::new(
            Diagnostic::from_error(&error).code.as_str(),
            "检查未完成，请在本机确认输入和工具状态；不会自动修改文件或重试。",
        )
    }
}

type CheckResponse = Result<CheckResult, CheckError>;

pub(crate) fn run_doctor(args: DoctorArgs, has_config: bool) -> i32 {
    emit("doctor", has_config, || doctor::check(args))
}

pub(crate) fn run_preflight(args: PreflightArgs, has_config: bool) -> i32 {
    emit("preflight", has_config, || preflight::check(args))
}

pub(crate) fn run_plan_protect(args: PlanProtectArgs, config: Option<&Path>) -> i32 {
    // 该入口明确消费本地配置，不使用旧只读检查的配置拒绝策略。
    emit("plan-protect", false, || plan::check(args, config))
}

fn emit(operation: &str, has_config: bool, check: impl FnOnce() -> CheckResponse) -> i32 {
    let result = if has_config {
        Err(CheckError::new(
            "CONFIG_NOT_SUPPORTED",
            "只读检查暂不接受 --config，请显式提供本次检查参数。",
        ))
    } else {
        check()
    };
    let mut response = json!({
        "schema_version": 1,
        "tool_version": env!("CARGO_PKG_VERSION"),
        "operation": operation,
    });
    let code = match result {
        Ok(result) => {
            response["type"] = json!("result");
            response["status"] = json!(result.status);
            response["data"] = result.data;
            i32::from(result.status == Status::Blocked)
        }
        Err(error) => {
            response["type"] = json!("error");
            response["code"] = json!(error.code);
            response["message"] = json!(error.message);
            1
        }
    };
    println!("{response}");
    code
}

fn require_file(path: &Path, not_file_code: &'static str) -> Result<(), CheckError> {
    let metadata = path
        .metadata()
        .map_err(|error| CheckError::from_core(error.into()))?;
    if !metadata.is_file() {
        return Err(CheckError::new(not_file_code, "指定位置不是普通文件。"));
    }
    File::open(path).map_err(|error| CheckError::from_core(error.into()))?;
    Ok(())
}

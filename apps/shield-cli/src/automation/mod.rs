//! 自动化适配：默认只有只读入口；隔离执行须显式启用开发特性，不读取 GUI 数据。
mod args;
mod doctor;
mod plan;
mod preflight;
// 默认只在测试中使用；macOS 共包入口显式启用执行特性，既有公开版尚不包含。
#[cfg(all(unix, any(test, all(feature = "agent-executor", target_os = "macos"))))]
mod publication;
mod resources;
mod snapshot;
// 协议与生命周期保持私有，正常构建不开放工作进程入口。
#[cfg(all(test, unix))]
mod executor_contract_tests;
#[cfg(all(target_os = "macos", any(test, feature = "agent-executor")))]
mod native_group;
#[cfg(any(test, all(feature = "agent-executor", target_os = "macos")))]
mod task_lifecycle;
#[cfg(all(unix, any(test, all(feature = "agent-executor", target_os = "macos"))))]
mod task_materials;
#[cfg(any(test, all(feature = "agent-executor", target_os = "macos")))]
mod worker_protocol;

#[cfg(all(feature = "agent-executor", target_os = "macos"))]
pub(crate) mod execution;

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
}

impl CheckError {
    fn new(code: &'static str, message: &'static str) -> Self {
        Self { code, message }
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

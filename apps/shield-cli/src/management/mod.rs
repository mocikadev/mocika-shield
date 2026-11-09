//! CLI 本地资料适配；应用库拥有存储和业务规则，不经 GUI／MCP 转发。
mod args;
mod certificates;
mod config;
mod credentials;
pub(crate) mod signing;

pub(crate) use args::{CertificateArgs, ConfigArgs};
use serde_json::{json, Value};
use shield_app::{certificates::CertificateStoreState, paths::ApplicationPaths};
use std::{io::Write, path::Path};

struct ManagementError {
    code: &'static str,
    message: &'static str,
    change_state: &'static str,
}
impl ManagementError {
    fn new(code: &'static str, message: &'static str) -> Self {
        Self {
            code,
            message,
            change_state: "not_attempted",
        }
    }
    fn after_write(mut self) -> Self {
        // 保存可能已提交、但回读失败，不能诱导自动重试或声称已经回滚。
        self.change_state = "unconfirmed";
        self
    }
    fn store(_: String) -> Self {
        Self::new(
            "CERTIFICATE_STORE_FAILED",
            "证书库操作未完成，请检查本机数据和权限；不会切换到其他数据库。",
        )
    }
}
type Response = Result<Value, ManagementError>;

pub(crate) fn run_certificates(
    args: CertificateArgs,
    home: Option<&Path>,
    has_config: bool,
) -> i32 {
    emit("cert", || {
        reject_project_config(has_config)?;
        certificates::run(args.command, &paths(home)?)
    })
}
pub(crate) fn run_config(args: ConfigArgs, home: Option<&Path>, has_config: bool) -> i32 {
    emit("config", || {
        reject_project_config(has_config)?;
        config::run(args.command, &paths(home)?)
    })
}

pub(crate) fn unsupported_home() -> i32 {
    emit("context", || {
        Err(ManagementError::new(
            "APP_HOME_NOT_SUPPORTED",
            "当前命令尚未接入共享应用目录，不能静默忽略 --app-home。",
        ))
    })
}
fn reject_project_config(present: bool) -> Result<(), ManagementError> {
    if present {
        Err(ManagementError::new(
            "CONFIG_NOT_SUPPORTED",
            "资料管理操作不接受项目 --config；使用默认应用目录或明确的 --app-home。",
        ))
    } else {
        Ok(())
    }
}
fn paths(home: Option<&Path>) -> Result<ApplicationPaths, ManagementError> {
    home.map_or_else(ApplicationPaths::current_user, ApplicationPaths::isolated)
        .map_err(|_| {
            ManagementError::new(
                "APP_HOME_INVALID",
                "无法定位应用目录；隔离目录须为明确的绝对子目录，不会回退真实用户目录。",
            )
        })
}
fn writable_store(paths: &ApplicationPaths) -> Result<CertificateStoreState, ManagementError> {
    CertificateStoreState::open_existing(paths.certificate_database(), paths.keystore_dir())
        .map_err(ManagementError::store)
}
fn initialize_store(paths: &ApplicationPaths) -> Result<CertificateStoreState, ManagementError> {
    CertificateStoreState::initialize(paths.certificate_database(), paths.keystore_dir())
        .map_err(|error| ManagementError::store(error).after_write())
}
fn emit(operation: &str, run: impl FnOnce() -> Response) -> i32 {
    let mut value = json!({
        "schema_version": 1, "tool_version": env!("CARGO_PKG_VERSION"), "operation": operation,
    });
    let exit = match run() {
        Ok(data) => {
            value["type"] = json!("result");
            value["status"] = json!("ready");
            value["data"] = data;
            0
        }
        Err(error) => {
            value["type"] = json!("error");
            value["code"] = json!(error.code);
            value["message"] = json!(error.message);
            value["change_state"] = json!(error.change_state);
            1
        }
    };
    // 输出失败不能回滚已完成的持久化；本次管理命令不自动重试。
    if writeln!(std::io::stdout().lock(), "{value}").is_err() {
        1
    } else {
        exit
    }
}

use super::args::Operation;
use super::{require_file, CheckError, CheckResponse, CheckResult, DoctorArgs, Status};
use serde::Serialize;
use serde_json::json;
use shield_core::utils::{
    find_apksigner, find_apktool, find_runtime_resources, probe_java_environment,
    JavaEnvironmentInfo,
};
use std::path::PathBuf;

#[derive(Serialize)]
struct DependencyCheck {
    dependency: &'static str,
    status: Status,
    code: &'static str,
    verification: &'static str,
}

pub(super) fn check(args: DoctorArgs) -> CheckResponse {
    if (matches!(args.operation, Operation::Sign | Operation::Certificate)
        && (args.apktool.is_some() || args.resources.is_some()))
        || (matches!(args.operation, Operation::Certificate) && args.apksigner.is_some())
    {
        return Err(CheckError::new(
            "ARGUMENT_NOT_APPLICABLE",
            "本次操作不使用所指定的工具参数，请移除无关参数后重新检查。",
        ));
    }
    let java = probe_java_environment();
    let mut checks = vec![java_check(&java)];
    match args.operation {
        Operation::Protect => {
            checks.push(file_check("apktool", args.apktool, find_apktool));
            checks.push(file_check("apksigner", args.apksigner, find_apksigner));
            checks.push(file_check(
                "resources",
                args.resources,
                find_runtime_resources,
            ));
        }
        Operation::Sign => {
            checks.push(file_check("apksigner", args.apksigner, find_apksigner));
        }
        Operation::Certificate => checks.push(DependencyCheck {
            dependency: "keytool",
            status: if java.keytool_ready() {
                Status::Ready
            } else {
                Status::Blocked
            },
            code: if java.keytool_ready() {
                "KEYTOOL_LOCATED"
            } else {
                "TOOL_NOT_FOUND"
            },
            verification: "executable_located",
        }),
    }
    let status = if checks.iter().any(|check| check.status == Status::Blocked) {
        Status::Blocked
    } else {
        Status::Ready
    };
    let operation = match args.operation {
        Operation::Protect => "protect",
        Operation::Sign => "sign",
        Operation::Certificate => "certificate",
    };
    Ok(CheckResult {
        status,
        data: json!({
            "scope": "dependency_presence",
            "requested_operation": operation,
            "java_major": java.major_version,
            "checks": checks,
        }),
    })
}

fn java_check(java: &JavaEnvironmentInfo) -> DependencyCheck {
    let code = if java.java_path.is_none() {
        "JAVA_NOT_FOUND"
    } else if java.major_version.is_none() {
        "JAVA_PROBE_FAILED"
    } else if !java.java_ready() {
        "JAVA_UNSUPPORTED"
    } else {
        "JAVA_READY"
    };
    DependencyCheck {
        dependency: "java",
        status: if java.java_ready() {
            Status::Ready
        } else {
            Status::Blocked
        },
        code,
        verification: "java_version",
    }
}

fn file_check(
    dependency: &'static str,
    explicit: Option<PathBuf>,
    locate: impl FnOnce() -> anyhow::Result<PathBuf>,
) -> DependencyCheck {
    let path = explicit.map(Ok).unwrap_or_else(locate);
    let available = path.is_ok_and(|path| require_file(&path, "TOOL_NOT_FILE").is_ok());
    DependencyCheck {
        dependency,
        status: if available {
            Status::Ready
        } else {
            Status::Blocked
        },
        code: if available {
            "FILE_READABLE"
        } else {
            "FILE_UNAVAILABLE"
        },
        verification: "readable_file",
    }
}

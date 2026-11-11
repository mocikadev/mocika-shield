//! 桌面开发特性的受控自动化入口；内部工作消息不是跨版本服务协议。
use super::args::Profile;
use super::{CheckError, PlanProtectArgs};
use clap::Args;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::time::Instant;

mod channel;
mod controller;
use super::executable as identity;
use super::private_json::{read as read_json, write as write_json};
mod interface;
mod launch;
mod sources;
mod worker;
pub(crate) use interface::{run_agent, AgentArgs};

/// MCP 只复用绑定的安全解析，不读取或公开材料。
#[cfg(feature = "mcp-server")]
pub(crate) fn shared_binding_certificate(value: &str) -> Result<String, &'static str> {
    super::shared_plan::Binding::decode(value)
        .map(|binding| binding.certificate_id().to_owned())
        .map_err(|_| "CLI_PROTOCOL_INVALID")
}

#[derive(Clone, Copy)]
enum RunMode {
    Probe,
    Plan,
    Protect([u8; 32]),
}

#[derive(Clone, Copy)]
enum Notice {
    Stage(crate::automation::task_lifecycle::Stage),
    Publishing,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PlanProof {
    format_version: u32,
    sources: sources::Sources,
    warning: bool,
}

#[derive(Args)]
pub(crate) struct ProbeArgs {
    #[arg(long)]
    request: PathBuf,
}
#[derive(Args)]
pub(crate) struct WorkerArgs {
    #[arg(long)]
    root: PathBuf,
    #[arg(long)]
    task_id: String,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    config: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    shared: Option<super::shared_plan::SharedRequest>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    sign: Option<super::sign_plan::Request>,
    input: PathBuf,
    output: PathBuf,
    runtime_profile: Profile,
    exclude_abis: Vec<String>,
    timeout_seconds: u64,
}

impl Request {
    fn validate(&mut self) -> Result<(), CheckError> {
        if !(1..=3600).contains(&self.timeout_seconds)
            || self.exclude_abis.len() > 64
            || self.exclude_abis.iter().any(|abi| abi.len() > 64)
        {
            return Err(error("INVALID_REQUEST"));
        }
        let current = std::env::current_dir().map_err(|_| error("INVALID_REQUEST"))?;
        for path in self
            .config
            .iter_mut()
            .chain([&mut self.input, &mut self.output])
        {
            if path.is_relative() {
                *path = current.join(&*path);
            }
        }
        if self.config.is_none() && self.shared.is_none() && self.sign.is_none() {
            return Err(error("SIGNING_CONFIG_REQUIRED"));
        }
        if let Some(shared) = &self.shared {
            shared.validate()?;
        }
        if let Some(sign) = &self.sign {
            if self.shared.is_some()
                || !self.exclude_abis.is_empty()
                || self.runtime_profile != Profile::Standard
            {
                return Err(error("INVALID_REQUEST"));
            }
            sign.validate()?;
        }
        Ok(())
    }
    fn bootstrap_version(&self) -> u32 {
        if cfg!(windows) {
            return if self.sign.is_some() { 6 } else { 5 };
        }
        if self.sign.is_some() {
            4
        } else if self.shared.is_some() {
            3
        } else {
            2
        }
    }
    fn proof_version(&self) -> u32 {
        if cfg!(windows) {
            return if self.sign.is_some() { 5 } else { 4 };
        }
        if self.sign.is_some() {
            3
        } else if self.shared.is_some() {
            2
        } else {
            1
        }
    }
    fn plan_args(&self) -> PlanProtectArgs {
        PlanProtectArgs {
            input: self.input.clone(),
            output: self.output.clone(),
            runtime_profile: self.runtime_profile,
            exclude_abis: self.exclude_abis.clone(),
            json: true,
        }
    }
}

#[derive(Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Mode {
    Plan,
    Execute,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Bootstrap {
    format_version: u32,
    mode: Mode,
    request: Request,
    executor: identity::Executable,
    tools: Option<identity::Tools>,
}

pub(crate) fn run_probe(args: ProbeArgs, has_config: bool) -> i32 {
    let result = if has_config {
        Err(error("CONFIG_NOT_SUPPORTED"))
    } else {
        read_json::<Request>(&args.request, 16384).and_then(|(mut request, _)| {
            request.validate()?;
            controller::run(request, Arc::new(AtomicBool::new(false)))
        })
    };
    let (code, value) = match result {
        Ok(value) => (if value["status"] == "succeeded" { 0 } else { 1 }, value),
        Err(error) => (
            1,
            serde_json::json!({ "type":"error", "status":match error.code { "TASK_TIMEOUT" => "timed_out", "TASK_CANCELLED" => "cancelled", code if code.contains("UNCONFIRMED") => "unknown", _ => "failed" }, "code":error.code, "message":error.message, "retryable":false }),
        ),
    };
    let mut value = value;
    value["schema_version"] = serde_json::json!(1);
    value["experimental"] = serde_json::json!(true);
    value["tool_version"] = serde_json::json!(env!("CARGO_PKG_VERSION"));
    println!("{value}");
    code
}

pub(crate) fn run_worker(args: WorkerArgs, has_config: bool) -> i32 {
    if has_config {
        return 1;
    }
    worker::run(args)
}

fn error(code: &'static str) -> CheckError {
    CheckError::new(
        code,
        "开发验证未完成；请在本机核对，不会回传原始日志或自动重试。",
    )
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}
fn parse_hex<const N: usize>(value: &str) -> Result<[u8; N], CheckError> {
    if value.len() != N * 2 || !value.is_ascii() {
        return Err(error("INVALID_DIGEST"));
    }
    let mut bytes = [0u8; N];
    for (slot, pair) in bytes.iter_mut().zip(value.as_bytes().as_chunks::<2>().0) {
        let pair = std::str::from_utf8(pair).map_err(|_| error("INVALID_DIGEST"))?;
        *slot = u8::from_str_radix(pair, 16).map_err(|_| error("INVALID_DIGEST"))?;
    }
    Ok(bytes)
}

fn checkpoint(deadline: Instant, cancel: &AtomicBool) -> Result<(), CheckError> {
    if Instant::now() >= deadline {
        Err(error("TASK_TIMEOUT"))
    } else if cancel.load(Ordering::SeqCst) {
        Err(error("TASK_CANCELLED"))
    } else {
        Ok(())
    }
}

fn install_alignment(path: &Path) -> Result<bool, CheckError> {
    let issues = shield_core::verify_apk_alignment(path).map_err(|_| error("ARTIFACT_INVALID"))?;
    let file = std::fs::File::open(path).map_err(|_| error("ARTIFACT_INVALID"))?;
    let mut archive = zip::ZipArchive::new(file).map_err(|_| error("ARTIFACT_INVALID"))?;
    for issue in issues {
        let entry = archive
            .by_name(&issue.name)
            .map_err(|_| error("ARTIFACT_INVALID"))?;
        // 安装对齐约束针对未压缩内容；签名器追加的压缩签名条目不要求偏移对齐。
        // 复用核心偏移与 16 KB 规则，不在签名后重写 ZIP，以免破坏 V2/V3 签名。
        if entry.compression() == zip::CompressionMethod::Stored {
            return Ok(false);
        }
    }
    Ok(true)
}

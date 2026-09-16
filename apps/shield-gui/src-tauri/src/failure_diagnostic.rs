//! 将任务失败映射为受控诊断；不负责发送或持久化原始错误。
use serde::{Deserialize, Serialize};
use shield_core::diagnostic::{Diagnostic, FailureCode, ToolName, CLASSIFIER_VERSION};

#[derive(Clone, Debug)]
pub(crate) struct ExecutionFailure {
    pub(crate) message: String,
    pub(crate) diagnostic: Diagnostic,
    pub(crate) cancelled: bool,
    /// 空字段等前置输入校验不属于已执行操作失败。
    pub(crate) executed: bool,
}

impl ExecutionFailure {
    pub(crate) fn diagnosed(message: String, diagnostic: Diagnostic) -> Self {
        Self {
            message,
            diagnostic,
            cancelled: false,
            executed: true,
        }
    }
    pub(crate) fn preflight(message: String, diagnostic: Diagnostic) -> Self {
        Self {
            message,
            diagnostic,
            cancelled: false,
            executed: false,
        }
    }
    pub(crate) fn from_shield(error: shield_core::ShieldError) -> Self {
        let preflight = match &error {
            shield_core::ShieldError::Other(source) => {
                shield_core::diagnostic::is_preflight_error(source)
            }
            _ => false,
        };
        match Diagnostic::from_shield(&error) {
            Some(diagnostic) if preflight => Self::preflight(error.to_string(), diagnostic),
            Some(diagnostic) => Self::diagnosed(error.to_string(), diagnostic),
            None => Self {
                message: "已取消".into(),
                diagnostic: Diagnostic::new(FailureCode::Unknown),
                cancelled: true,
                executed: false,
            },
        }
    }
}
impl From<String> for ExecutionFailure {
    fn from(message: String) -> Self {
        Self {
            message,
            diagnostic: Diagnostic::new(FailureCode::Unknown),
            cancelled: false,
            executed: true,
        }
    }
}
impl std::fmt::Display for ExecutionFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.message.fmt(f)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Flow {
    Protect,
    ProtectWithSign,
    Sign,
    Certificate,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Operation {
    Protect,
    Sign,
    Certificate,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Stage {
    Prepare,
    Unpack,
    Manifest,
    DexRuntime,
    Align,
    Execute,
    Sign,
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct FailureContext {
    pub(crate) flow: Flow,
    pub(crate) operation: Operation,
    pub(crate) stage: Stage,
}
impl FailureContext {
    pub(crate) fn protect(step: &str, signing_started: bool, auto_sign: bool) -> Self {
        let flow = if auto_sign {
            Flow::ProtectWithSign
        } else {
            Flow::Protect
        };
        if signing_started {
            return Self {
                flow,
                ..Self::sign(step)
            };
        }
        let stage = match step {
            "CheckTools" => Stage::Prepare,
            "Unpack" => Stage::Unpack,
            "ModifyManifest" | "Repack" => Stage::Manifest,
            "ProcessDex" | "InjectRuntime" => Stage::DexRuntime,
            "AlignApk" => Stage::Align,
            _ => Stage::Unknown,
        };
        Self {
            flow,
            operation: Operation::Protect,
            stage,
        }
    }
    pub(crate) fn sign(step: &str) -> Self {
        Self {
            flow: Flow::Sign,
            operation: Operation::Sign,
            stage: match step {
                "PrepareSign" => Stage::Prepare,
                "AlignApk" => Stage::Align,
                "SignApk" | "Cleanup" => Stage::Execute,
                _ => Stage::Unknown,
            },
        }
    }
    pub(crate) fn certificate() -> Self {
        Self {
            flow: Flow::Certificate,
            operation: Operation::Certificate,
            stage: Stage::Execute,
        }
    }
}

/// 只有此有限结构可进入报告缓存，不能从前端反序列化构造。
#[derive(Clone, Debug, Serialize)]
pub(crate) struct SafeReport {
    pub(crate) report_id: String,
    pub(crate) schema_version: u32,
    pub(crate) classifier_version: u32,
    pub(crate) sanitizer_version: u32,
    pub(crate) app_version: &'static str,
    pub(crate) build_revision: Option<&'static str>,
    pub(crate) build_kind: &'static str,
    pub(crate) occurred_at: u64,
    #[serde(flatten)]
    pub(crate) context: FailureContext,
    pub(crate) code: FailureCode,
    pub(crate) platform: &'static str,
    pub(crate) arch: &'static str,
    pub(crate) java_major: Option<u32>,
    pub(crate) java_vendor: Option<&'static str>,
    pub(crate) tool_name: Option<ToolName>,
    pub(crate) tool_version: Option<&'static str>,
    pub(crate) exit_code: Option<i32>,
    pub(crate) evidence: Vec<()>,
}
impl SafeReport {
    pub(crate) fn new(context: FailureContext, diagnostic: Diagnostic, occurred_at: u64) -> Self {
        Self {
            report_id: uuid::Uuid::new_v4().to_string(),
            schema_version: 1,
            classifier_version: CLASSIFIER_VERSION,
            sanitizer_version: 1,
            app_version: env!("CARGO_PKG_VERSION"),
            build_revision: option_env!("GIT_HASH").filter(|hash| {
                (7..=40).contains(&hash.len())
                    && hash
                        .bytes()
                        .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
            }),
            build_kind: "unknown",
            occurred_at,
            context,
            code: diagnostic.code,
            platform: match std::env::consts::OS {
                "macos" => "macos",
                "windows" => "windows",
                "linux" => "linux",
                _ => "unknown",
            },
            arch: match std::env::consts::ARCH {
                "aarch64" => "aarch64",
                "x86_64" => "x86_64",
                "x86" => "x86",
                "arm" => "arm",
                _ => "unknown",
            },
            java_major: None,
            java_vendor: None,
            tool_name: diagnostic.tool,
            tool_version: None,
            exit_code: diagnostic.exit_code,
            evidence: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct FailureReasonCount {
    #[serde(flatten)]
    pub(crate) context: FailureContext,
    pub(crate) code: FailureCode,
    pub(crate) classifier_version: u32,
    pub(crate) count: u32,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn 字符串边界错误默认是已执行失败() {
        let failure = ExecutionFailure::from("后台任务执行失败".to_string());
        assert!(failure.executed);
    }

    #[test]
    fn 预检失败不是已执行失败() {
        let failure = ExecutionFailure::preflight(
            "APK 架构不受支持".to_string(),
            Diagnostic::new(FailureCode::UnsupportedAbi),
        );
        assert!(!failure.executed);
        assert_eq!(failure.diagnostic.code, FailureCode::UnsupportedAbi);
    }
    #[test]
    fn 报告与跨端协议样本一致() {
        let mut report = SafeReport::new(
            FailureContext::sign("SignApk"),
            Diagnostic::new(FailureCode::SigningFailed),
            1789516800,
        );
        report.report_id = "7acbe71f-6a86-43bc-8dcd-1c710f196dad".into();
        report.build_revision = None;
        report.platform = "macos";
        report.arch = "aarch64";
        let mut expected: serde_json::Value = serde_json::from_str(include_str!(
            "../../../../tests/fixtures/diagnostics/error-report-v1.json"
        ))
        .unwrap();
        expected["app_version"] = serde_json::Value::String(env!("CARGO_PKG_VERSION").into());
        assert_eq!(report.app_version, env!("CARGO_PKG_VERSION"));
        assert_eq!(serde_json::to_value(report).unwrap(), expected);
    }

    #[test]
    fn 敏感样本不进入任何报告字段() {
        for raw in [
            "/Users/private/客户.apk pass:口令",
            "C:\\Users\\private\\签名.jks --ks-pass secret",
            "\\\\?\\UNC\\server\\private\\app.apk",
            "CN=秘密客户 alias=中文证书 SHA256:AA:BB com.company.secret",
            "https://example.invalid/?token=PRIVATE_TOKEN java.lang.Exception: secret",
        ] {
            let diagnostic =
                Diagnostic::from_tool_output(ToolName::Apksigner, Some(1), raw.as_bytes());
            let report = SafeReport::new(FailureContext::sign("SignApk"), diagnostic, 100);
            let json = serde_json::to_string(&report).unwrap();
            for secret in [
                "private",
                "secret",
                "PRIVATE_TOKEN",
                "口令",
                "客户",
                "中文证书",
                "com.company",
            ] {
                assert!(!json.contains(secret), "敏感样本不应出现在报告中");
            }
            assert!(report.evidence.is_empty());
            assert!(json.len() < 8192);
        }
    }

    #[test]
    fn 自动签名上下文与加固对齐分离() {
        let protect = FailureContext::protect("AlignApk", false, true);
        assert_eq!(protect.operation, Operation::Protect);
        let sign = FailureContext::protect("AlignApk", true, true);
        assert_eq!(sign.operation, Operation::Sign);
        assert_eq!(sign.flow, Flow::ProtectWithSign);
        assert_eq!(sign.stage, Stage::Align);
    }

    #[test]
    fn 快照冻结版本且未知错误不透传() {
        let failure = ExecutionFailure::from("秘密密码 /Users/private/app.apk".to_string());
        let report = SafeReport::new(FailureContext::sign("SignApk"), failure.diagnostic, 1234);
        let json = serde_json::to_string(&report).unwrap();
        assert!(!json.contains("秘密"));
        assert!(!json.contains("private"));
        assert_eq!(report.app_version, env!("CARGO_PKG_VERSION"));
        assert_eq!(report.occurred_at, 1234);
        assert_eq!(report.code, FailureCode::Unknown);
    }
}

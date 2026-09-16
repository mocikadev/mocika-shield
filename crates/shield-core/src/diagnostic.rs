//! 有限错误分类；不保存原始输出，不依赖 GUI、网络或持久化协议。
use crate::ShieldError;
use std::fmt;

pub const CLASSIFIER_VERSION: u32 = 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum FailureCode {
    ToolNotFound,
    JavaUnsupported,
    FileNotFound,
    PermissionDenied,
    DiskFull,
    UnsupportedAbi,
    KeystorePasswordInvalid,
    KeyAliasNotFound,
    KeystoreFormatInvalid,
    ToolOutputEncodingInvalid,
    ManifestRebuildFailed,
    ApkRepackFailed,
    RuntimeInjectionFailed,
    AlignmentFailed,
    SigningFailed,
    ToolProcessFailed,
    Unknown,
}

impl FailureCode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ToolNotFound => "TOOL_NOT_FOUND",
            Self::JavaUnsupported => "JAVA_UNSUPPORTED",
            Self::FileNotFound => "FILE_NOT_FOUND",
            Self::PermissionDenied => "PERMISSION_DENIED",
            Self::DiskFull => "DISK_FULL",
            Self::UnsupportedAbi => "UNSUPPORTED_ABI",
            Self::KeystorePasswordInvalid => "KEYSTORE_PASSWORD_INVALID",
            Self::KeyAliasNotFound => "KEY_ALIAS_NOT_FOUND",
            Self::KeystoreFormatInvalid => "KEYSTORE_FORMAT_INVALID",
            Self::ToolOutputEncodingInvalid => "TOOL_OUTPUT_ENCODING_INVALID",
            Self::ManifestRebuildFailed => "MANIFEST_REBUILD_FAILED",
            Self::ApkRepackFailed => "APK_REPACK_FAILED",
            Self::RuntimeInjectionFailed => "RUNTIME_INJECTION_FAILED",
            Self::AlignmentFailed => "ALIGNMENT_FAILED",
            Self::SigningFailed => "SIGNING_FAILED",
            Self::ToolProcessFailed => "TOOL_PROCESS_FAILED",
            Self::Unknown => "UNKNOWN",
        }
    }

    pub fn should_prompt(self) -> bool {
        matches!(
            self,
            Self::ToolOutputEncodingInvalid
                | Self::ManifestRebuildFailed
                | Self::ApkRepackFailed
                | Self::RuntimeInjectionFailed
                | Self::AlignmentFailed
                | Self::SigningFailed
                | Self::ToolProcessFailed
                | Self::Unknown
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ToolName {
    Java,
    Keytool,
    Apktool,
    Apksigner,
    Zipalign,
}

impl ToolName {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Java => "java",
            Self::Keytool => "keytool",
            Self::Apktool => "apktool",
            Self::Apksigner => "apksigner",
            Self::Zipalign => "zipalign",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Diagnostic {
    pub code: FailureCode,
    pub tool: Option<ToolName>,
    pub exit_code: Option<i32>,
}

impl Diagnostic {
    pub fn new(code: FailureCode) -> Self {
        Self {
            code,
            tool: None,
            exit_code: None,
        }
    }

    pub fn from_shield(error: &ShieldError) -> Option<Self> {
        Some(match error {
            ShieldError::Cancelled => return None,
            ShieldError::ToolNotFound(_) => Self::new(FailureCode::ToolNotFound),
            ShieldError::FileNotFound(_) => Self::new(FailureCode::FileNotFound),
            ShieldError::CommandFailed(_) => Self::new(FailureCode::ToolProcessFailed),
            ShieldError::Other(error) => Self::from_error(error),
            _ => Self::new(FailureCode::Unknown),
        })
    }

    pub fn from_error(error: &anyhow::Error) -> Self {
        for cause in error.chain() {
            if let Some(typed) = cause.downcast_ref::<DiagnosedError>() {
                return typed.diagnostic;
            }
            if let Some(io) = cause.downcast_ref::<std::io::Error>() {
                return Self::from_io(io);
            }
            if let Some(shield) = cause.downcast_ref::<ShieldError>() {
                if let Some(diagnostic) = Self::from_shield(shield) {
                    return diagnostic;
                }
            }
        }
        Self::new(FailureCode::Unknown)
    }

    pub fn from_io(error: &std::io::Error) -> Self {
        Self::new(match error.kind() {
            std::io::ErrorKind::NotFound => FailureCode::FileNotFound,
            std::io::ErrorKind::PermissionDenied => FailureCode::PermissionDenied,
            std::io::ErrorKind::StorageFull => FailureCode::DiskFull,
            _ => FailureCode::Unknown,
        })
    }

    /// 仅在工具边界调用，关键词只改变有限枚举；绝不保留输入字节。
    pub fn from_tool_output(tool: ToolName, exit_code: Option<i32>, bytes: &[u8]) -> Self {
        let mut result = Self {
            code: FailureCode::ToolProcessFailed,
            tool: Some(tool),
            exit_code,
        };
        let Ok(text) = std::str::from_utf8(bytes) else {
            result.code = FailureCode::ToolOutputEncodingInvalid;
            return result;
        };
        let lower = text.to_ascii_lowercase();
        result.code = if lower.contains("java.lang.unsupportedclassversionerror")
            || lower.contains("unsupported major.minor version")
        {
            FailureCode::JavaUnsupported
        } else if matches!(tool, ToolName::Keytool | ToolName::Apksigner) {
            if lower.contains("password was incorrect")
                || lower.contains("password is incorrect")
                || lower.contains("cannot recover key")
            {
                FailureCode::KeystorePasswordInvalid
            } else if lower.contains("no key with alias")
                || (lower.contains("alias") && lower.contains("does not exist"))
            {
                FailureCode::KeyAliasNotFound
            } else if lower.contains("invalid keystore format")
                || lower.contains("unrecognized keystore format")
                || lower.contains("toderinputstream rejects tag type")
            {
                FailureCode::KeystoreFormatInvalid
            } else {
                FailureCode::ToolProcessFailed
            }
        } else {
            FailureCode::ToolProcessFailed
        };
        result
    }

    /// 只补充未知类别，不能覆盖文件权限等已经确认的根因。
    pub fn or_code(mut self, code: FailureCode) -> Self {
        if self.code == FailureCode::Unknown {
            self.code = code;
        }
        self
    }

    pub fn attach(self, source: anyhow::Error) -> anyhow::Error {
        anyhow::Error::new(DiagnosedError {
            diagnostic: self,
            source,
            preflight: false,
        })
    }

    pub fn attach_preflight(self, source: anyhow::Error) -> anyhow::Error {
        anyhow::Error::new(DiagnosedError {
            diagnostic: self,
            source,
            preflight: true,
        })
    }
}

pub fn is_preflight_error(error: &anyhow::Error) -> bool {
    error.chain().any(|cause| {
        cause
            .downcast_ref::<DiagnosedError>()
            .is_some_and(|diagnosed| diagnosed.preflight)
    })
}

#[derive(Debug)]
struct DiagnosedError {
    diagnostic: Diagnostic,
    source: anyhow::Error,
    preflight: bool,
}
impl fmt::Display for DiagnosedError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.source, f)
    }
}
impl std::error::Error for DiagnosedError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(self.source.as_ref())
    }
}

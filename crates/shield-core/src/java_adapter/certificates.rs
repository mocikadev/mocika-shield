//! 有界只读证书结果、路径解析与有限错误映射；不访问资料库，不公开通用执行器。
use super::*;
use crate::diagnostic::ToolName;
use serde::de::DeserializeOwned;
use std::collections::HashSet;

const MAX: u64 = 65536;
const ALIASES: &str = "keystore_aliases";
const FINGERPRINT: &str = "keystore_fingerprint";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Success<T> {
    protocol_version: u32,
    application_version: String,
    operation: String,
    result: T,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Failure {
    protocol_version: u32,
    application_version: String,
    operation: String,
    error: String,
}
#[derive(Deserialize)]
#[serde(untagged)]
enum Reply<T> {
    Success(Success<T>),
    Failure(Failure),
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Aliases {
    aliases: Vec<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Fingerprint {
    fingerprint: String,
}

fn error(code: FailureCode, message: &'static str, status: Option<i32>) -> anyhow::Error {
    let mut diagnostic = Diagnostic::new(code);
    diagnostic.tool = Some(ToolName::Keytool);
    diagnostic.exit_code = status;
    diagnostic.attach(anyhow::anyhow!(message))
}

pub(super) fn invalid_result() -> anyhow::Error {
    error(
        FailureCode::ToolProcessFailed,
        "证书操作未返回匹配的完整结果；未回退旧参数重试。",
        None,
    )
}

pub(super) fn decode<T: DeserializeOwned>(
    bytes: &[u8],
    status: Option<i32>,
    operation: &str,
) -> Result<T> {
    if bytes.len() as u64 > MAX {
        return Err(invalid_result());
    }
    let reply: Reply<T> = serde_json::from_slice(bytes).map_err(|_| invalid_result())?;
    let (protocol, version, actual) = match &reply {
        Reply::Success(value) => (
            value.protocol_version,
            &value.application_version,
            &value.operation,
        ),
        Reply::Failure(value) => (
            value.protocol_version,
            &value.application_version,
            &value.operation,
        ),
    };
    if protocol != PROTOCOL || version != env!("CARGO_PKG_VERSION") || actual != operation {
        return Err(invalid_result());
    }
    match reply {
        Reply::Success(value) if status == Some(0) => Ok(value.result),
        Reply::Failure(value) if status == Some(4) => {
            let (code, message) = match value.error.as_str() {
                "PASSWORD_INVALID" => (FailureCode::KeystorePasswordInvalid, "Keystore 密码不正确"),
                "ALIAS_NOT_FOUND" => (
                    FailureCode::KeyAliasNotFound,
                    "Keystore 中没有可用或指定的 Alias",
                ),
                "FORMAT_INVALID" => (
                    FailureCode::KeystoreFormatInvalid,
                    "Keystore 类型不支持、格式不匹配或文件已损坏",
                ),
                "FILE_NOT_FOUND" => (FailureCode::FileNotFound, "找不到 Keystore 文件"),
                "PERMISSION_DENIED" => {
                    (FailureCode::PermissionDenied, "没有权限读取 Keystore 文件")
                }
                "CERTIFICATE_MISSING" => (
                    FailureCode::ToolProcessFailed,
                    "所选条目没有证书，无法读取签名指纹",
                ),
                "RESULT_LIMIT" => (
                    FailureCode::ToolProcessFailed,
                    "证书查询结果超出预算；未返回部分结果",
                ),
                "TOOL_FAILED" => (FailureCode::ToolProcessFailed, "证书只读查询失败"),
                "INPUT_INVALID" => (FailureCode::Unknown, "新建证书参数或核验结果不符合要求"),
                "SUBJECT_INVALID" => (
                    FailureCode::Unknown,
                    "证书主题格式不正确，请检查 CN、OU、O 等字段",
                ),
                "PASSWORD_UNSUPPORTED" => (
                    FailureCode::KeystorePasswordInvalid,
                    "PKCS12 需要 ASCII 密码，Key 密码必须留空或与 Keystore 密码相同",
                ),
                "OUTPUT_EXISTS" => (
                    FailureCode::ToolProcessFailed,
                    "暂存证书目标已存在，未覆盖原件",
                ),
                _ => return Err(invalid_result()),
            };
            Err(error(code, message, status))
        }
        _ => Err(invalid_result()),
    }
}

fn path(path: &Path) -> Result<String> {
    let absolute = crate::utils::strip_unc_prefix(&std::path::absolute(path)?);
    let metadata = std::fs::metadata(&absolute).map_err(|cause| {
        error(
            Diagnostic::from_io(&cause).code,
            "无法读取 Keystore 文件",
            None,
        )
    })?;
    if !metadata.is_file() {
        return Err(error(
            FailureCode::KeystoreFormatInvalid,
            "Keystore 必须是普通文件",
            None,
        ));
    }
    #[cfg(windows)]
    {
        use std::path::{Component, Prefix};
        if matches!(absolute.components().next(), Some(Component::Prefix(prefix)) if matches!(prefix.kind(),
            Prefix::Verbatim(_) | Prefix::VerbatimUNC(_, _) | Prefix::VerbatimDisk(_) | Prefix::DeviceNS(_)))
        {
            return Err(invalid_result());
        }
    }
    absolute
        .to_str()
        .filter(|value| !value.contains('\0'))
        .map(str::to_owned)
        .ok_or_else(invalid_result)
}

fn aliases(result: Aliases) -> Result<Vec<String>> {
    let mut unique = HashSet::new();
    if result.aliases.is_empty()
        || result.aliases.len() > 256
        || result
            .aliases
            .iter()
            .any(|alias| alias.len() > 4096 || !unique.insert(alias))
    {
        return Err(invalid_result());
    }
    Ok(result.aliases)
}

fn fingerprint(result: Fingerprint) -> Result<String> {
    if result.fingerprint.len() != 64
        || !result
            .fingerprint
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'A'..=b'F').contains(&byte))
    {
        return Err(invalid_result());
    }
    Ok(result.fingerprint)
}

impl JavaToolContext {
    pub(crate) fn certificate_aliases(
        &self,
        java: &Path,
        keystore: &Path,
        password: &str,
        kind: Option<&str>,
    ) -> Result<Vec<String>> {
        let path = path(keystore)?;
        let (status, bytes) = self.invoke(
            java,
            &[ALIASES, &path, kind.unwrap_or("JKS")],
            Some(password),
            MAX,
        )?;
        aliases(decode(&bytes, status.code(), ALIASES)?)
    }

    pub(crate) fn certificate_fingerprint(
        &self,
        java: &Path,
        keystore: &Path,
        alias: &str,
        password: &str,
        kind: Option<&str>,
    ) -> Result<String> {
        let path = path(keystore)?;
        let (status, bytes) = self.invoke(
            java,
            &[FINGERPRINT, &path, kind.unwrap_or(""), alias],
            Some(password),
            MAX,
        )?;
        fingerprint(decode(&bytes, status.code(), FINGERPRINT)?)
    }
}

#[cfg(test)]
mod tests;

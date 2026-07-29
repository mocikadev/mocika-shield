use anyhow::{Context, Result};

use crate::protect::native_alias::NativeAliasProtocol;

const RUNTIME_PROTOCOL: u32 = 2;
const CACHE_SCHEMA: u32 = 1;

#[derive(Debug, Clone)]
pub(crate) struct RuntimeMetadata {
    pub(crate) stub_application: String,
    pub(crate) native_alias: NativeAliasProtocol,
    pub(crate) environment_policy: bool,
}

impl RuntimeMetadata {
    pub(crate) fn parse(json: &str) -> Result<Self> {
        let runtime_protocol =
            parse_u32(json, "runtime_protocol").context("metadata.json 缺少 runtime_protocol")?;
        let cache_schema =
            parse_u32(json, "cache_schema").context("metadata.json 缺少 cache_schema")?;
        let environment_policy = parse_bool(json, "environment_policy")
            .context("metadata.json 缺少 environment_policy")?;
        let memory_dex = parse_bool(json, "memory_dex").context("metadata.json 缺少 memory_dex")?;
        if runtime_protocol != RUNTIME_PROTOCOL {
            anyhow::bail!("不支持的 Runtime 资源协议: {runtime_protocol}");
        }
        if cache_schema != CACHE_SCHEMA {
            anyhow::bail!("不支持的 DEX 缓存协议: {cache_schema}");
        }
        if memory_dex {
            anyhow::bail!("当前版本不支持启用内存 DEX 资源");
        }
        Ok(Self {
            stub_application: parse_string(json, "stub_application")
                .context("metadata.json 缺少 stub_application")?,
            native_alias: NativeAliasProtocol::parse(json)?,
            environment_policy,
        })
    }
}

fn value_after_key<'a>(json: &'a str, field: &str) -> Option<&'a str> {
    let needle = format!("\"{field}\"");
    let after_key = &json[json.find(&needle)? + needle.len()..];
    after_key
        .trim_start()
        .strip_prefix(':')
        .map(str::trim_start)
}

fn parse_string(json: &str, field: &str) -> Option<String> {
    let value = value_after_key(json, field)?.strip_prefix('"')?;
    Some(value[..value.find('"')?].to_string())
}

fn parse_u32(json: &str, field: &str) -> Option<u32> {
    let value = value_after_key(json, field)?;
    let end = value
        .find(|character: char| !character.is_ascii_digit())
        .unwrap_or(value.len());
    value[..end].parse().ok()
}

fn parse_bool(json: &str, field: &str) -> Option<bool> {
    let value = value_after_key(json, field)?;
    if value.starts_with("true") {
        Some(true)
    } else if value.starts_with("false") {
        Some(false)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const METADATA: &str = r#"{
        "stub_application":"msk.d",
        "native_library":"libmocikashield.so",
        "native_name_placeholder":"mocikanativeslot",
        "native_name_length":16,
        "native_name_scheme":1,
        "runtime_protocol":2,
        "cache_schema":1,
        "environment_policy":false,
        "memory_dex":false
    }"#;

    #[test]
    fn parses_supported_capabilities() {
        let metadata = RuntimeMetadata::parse(METADATA).unwrap();
        assert_eq!(metadata.stub_application, "msk.d");
        assert_eq!(metadata.native_alias.name_length, 16);
        assert!(!metadata.environment_policy);
    }

    #[test]
    fn rejects_unsupported_protocol_or_memory_dex() {
        assert!(RuntimeMetadata::parse(
            &METADATA.replace("\"runtime_protocol\":2", "\"runtime_protocol\":3")
        )
        .is_err());
        assert!(RuntimeMetadata::parse(
            &METADATA.replace("\"memory_dex\":false", "\"memory_dex\":true")
        )
        .is_err());
    }
}

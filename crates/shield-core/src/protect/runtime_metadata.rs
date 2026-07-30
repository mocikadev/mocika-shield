use anyhow::{Context, Result};

use crate::protect::native_alias::NativeAliasProtocol;

const STANDARD_RUNTIME_PROTOCOL: u32 = 2;
const MEMORY_RUNTIME_PROTOCOL: u32 = 3;
const CACHE_SCHEMA: u32 = 1;
const MEMORY_DEX_MIN_API: u32 = 29;

#[derive(Debug, Clone)]
pub(crate) struct RuntimeMetadata {
    pub(crate) stub_application: String,
    pub(crate) stub_component_factory: Option<String>,
    pub(crate) native_alias: NativeAliasProtocol,
    pub(crate) environment_policy: bool,
    pub(crate) memory_dex: bool,
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
        if cache_schema != CACHE_SCHEMA {
            anyhow::bail!("不支持的 DEX 缓存协议: {cache_schema}");
        }
        let stub_component_factory = match (runtime_protocol, memory_dex) {
            (STANDARD_RUNTIME_PROTOCOL, false) => None,
            (MEMORY_RUNTIME_PROTOCOL, true) => {
                let min_api = parse_u32(json, "memory_dex_min_api")
                    .context("metadata.json 缺少 memory_dex_min_api")?;
                if min_api != MEMORY_DEX_MIN_API {
                    anyhow::bail!("不支持的内存 DEX 最低 API: {min_api}");
                }
                Some(
                    parse_string(json, "stub_component_factory")
                        .context("metadata.json 缺少 stub_component_factory")?,
                )
            }
            (STANDARD_RUNTIME_PROTOCOL, true) | (MEMORY_RUNTIME_PROTOCOL, false) => {
                anyhow::bail!("Runtime 协议与内存 DEX 能力声明不一致");
            }
            _ => anyhow::bail!("不支持的 Runtime 资源协议: {runtime_protocol}"),
        };
        let stub_application = parse_string(json, "stub_application")
            .context("metadata.json 缺少 stub_application")?;
        if !is_java_class_name(&stub_application) {
            anyhow::bail!("metadata.json 的 stub_application 不是有效类名");
        }
        if let Some(factory) = stub_component_factory.as_deref() {
            if !is_java_class_name(factory) {
                anyhow::bail!("metadata.json 的 stub_component_factory 不是有效类名");
            }
        }
        Ok(Self {
            stub_application,
            stub_component_factory,
            native_alias: NativeAliasProtocol::parse(json)?,
            environment_policy,
            memory_dex,
        })
    }
}

fn is_java_class_name(value: &str) -> bool {
    value.split('.').all(|segment| {
        let mut characters = segment.chars();
        matches!(characters.next(), Some(first) if first.is_ascii_alphabetic() || first == '_' || first == '$')
            && characters.all(|character| {
                character.is_ascii_alphanumeric() || character == '_' || character == '$'
            })
    })
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
        assert!(!metadata.memory_dex);
        assert!(metadata.stub_component_factory.is_none());
    }

    #[test]
    fn rejects_unsupported_or_inconsistent_protocol() {
        assert!(RuntimeMetadata::parse(
            &METADATA.replace("\"runtime_protocol\":2", "\"runtime_protocol\":3")
        )
        .is_err());
        assert!(RuntimeMetadata::parse(
            &METADATA.replace("\"memory_dex\":false", "\"memory_dex\":true")
        )
        .is_err());
    }

    #[test]
    fn parses_memory_candidate_protocol() {
        let candidate = METADATA
            .replace("\"runtime_protocol\":2", "\"runtime_protocol\":3")
            .replace(
                "\"memory_dex\":false",
                "\"memory_dex\":true,\n        \"memory_dex_min_api\":29,\n        \"stub_component_factory\":\"msk.f\"",
            );
        let metadata = RuntimeMetadata::parse(&candidate).unwrap();
        assert!(metadata.memory_dex);
        assert_eq!(metadata.stub_component_factory.as_deref(), Some("msk.f"));
    }

    #[test]
    fn rejects_invalid_candidate_factory_name() {
        let candidate = METADATA
            .replace("\"runtime_protocol\":2", "\"runtime_protocol\":3")
            .replace(
                "\"memory_dex\":false",
                "\"memory_dex\":true,\n        \"memory_dex_min_api\":29,\n        \"stub_component_factory\":\"invalid factory\"",
            );
        assert!(RuntimeMetadata::parse(&candidate).is_err());
    }
}

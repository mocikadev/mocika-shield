use anyhow::{Context, Result};

use crate::protect::manifest_keys::{self, ManifestKeys};
use crate::protect::native_alias::NativeAliasProtocol;

const CACHE_SCHEMA: u32 = 1;
const MEMORY_DEX_MIN_API: u32 = 31;

#[derive(Debug, Clone)]
pub(crate) struct RuntimeMetadata {
    pub(crate) stub_application: String,
    pub(crate) stub_component_factory: Option<String>,
    pub(crate) native_alias: NativeAliasProtocol,
    pub(crate) environment_policy: bool,
    pub(crate) memory_dex: bool,
    pub(crate) min_android_api: u32,
    pub(crate) manifest_keys: &'static ManifestKeys,
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
        // 老资源未声明最低 API 时沿用标准资源，不把缺字段推测为 API 19。
        let min_android_api = if json.contains("\"min_android_api\"") {
            parse_u32(json, "min_android_api").context("metadata.json 的 min_android_api 无效")?
        } else {
            21
        };
        if min_android_api == 19 && memory_dex {
            anyhow::bail!("API 19 兼容资源不能声明内存 DEX");
        }
        if cache_schema != CACHE_SCHEMA {
            anyhow::bail!("不支持的 DEX 缓存协议: {cache_schema}");
        }
        let (manifest_keys, expects_memory) = match runtime_protocol {
            manifest_keys::LEGACY_STANDARD_PROTOCOL => (&manifest_keys::LEGACY, false),
            manifest_keys::LEGACY_MEMORY_PROTOCOL => (&manifest_keys::LEGACY, true),
            manifest_keys::STANDARD_PROTOCOL => (&manifest_keys::CURRENT, false),
            manifest_keys::MEMORY_PROTOCOL => (&manifest_keys::CURRENT, true),
            _ => {
                anyhow::bail!("不支持的 Runtime 资源协议: {runtime_protocol}，请使用配套工具与资源")
            }
        };
        if memory_dex != expects_memory {
            anyhow::bail!("Runtime 协议与内存 DEX 能力声明不一致");
        }
        let stub_component_factory = if memory_dex {
            let min_api = parse_u32(json, "memory_dex_min_api")
                .context("metadata.json 缺少 memory_dex_min_api")?;
            if min_api != MEMORY_DEX_MIN_API {
                anyhow::bail!("不支持的内存 DEX 最低 API: {min_api}");
            }
            Some(
                parse_string(json, "stub_component_factory")
                    .context("metadata.json 缺少 stub_component_factory")?,
            )
        } else {
            None
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
            min_android_api,
            manifest_keys,
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
    let mut values = json.match_indices(&needle).filter_map(|(offset, _)| {
        json[offset + needle.len()..]
            .trim_start()
            .strip_prefix(':')
            .map(str::trim_start)
    });
    let value = values.next()?;
    if values.next().is_some() {
        return None;
    }
    Some(value)
}

fn parse_string(json: &str, field: &str) -> Option<String> {
    let value = value_after_key(json, field)?.strip_prefix('"')?;
    let end = value.find('"')?;
    scalar_end(&value[end + 1..]).then(|| value[..end].to_string())
}

fn parse_u32(json: &str, field: &str) -> Option<u32> {
    let value = value_after_key(json, field)?;
    let end = value
        .find(|character: char| !character.is_ascii_digit())
        .unwrap_or(value.len());
    scalar_end(&value[end..])
        .then(|| value[..end].parse().ok())
        .flatten()
}

fn parse_bool(json: &str, field: &str) -> Option<bool> {
    let value = value_after_key(json, field)?;
    if value.strip_prefix("true").is_some_and(scalar_end) {
        Some(true)
    } else if value.strip_prefix("false").is_some_and(scalar_end) {
        Some(false)
    } else {
        None
    }
}

fn scalar_end(value: &str) -> bool {
    matches!(value.trim_start().chars().next(), Some(',' | '}'))
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
        assert_eq!(metadata.min_android_api, 21);
        assert_eq!(metadata.manifest_keys.application, "ORIGINAL_APPLICATION");
    }

    #[test]
    fn 新旧协议选择配套字段且拒绝未知协议与能力错配() {
        for protocol in [2, 3, 4, 5, 6] {
            for memory in [false, true] {
                let json = METADATA.replace("\"runtime_protocol\":2", &format!("\"runtime_protocol\":{protocol}"))
                    .replace("\"memory_dex\":false", &format!("\"memory_dex\":{memory},\"memory_dex_min_api\":31,\"stub_component_factory\":\"msk.f\""));
                let result = RuntimeMetadata::parse(&json);
                if matches!((protocol, memory), (2 | 4, false) | (3 | 5, true)) {
                    let metadata = result.unwrap();
                    assert_eq!(
                        metadata.manifest_keys.policy,
                        if protocol <= 3 {
                            "dev.mocika.shield.ENV_POLICY"
                        } else {
                            "rt.policy"
                        }
                    );
                } else {
                    assert!(result.is_err());
                }
            }
        }
    }

    #[test]
    fn 拒绝歧义和错误类型的资源契约声明() {
        for (from, to) in [
            (
                "\"runtime_protocol\":2",
                "\"runtime_protocol\":4,\"runtime_protocol\":2",
            ),
            ("\"runtime_protocol\":2", "\"runtime_protocol\":4.1"),
            ("\"runtime_protocol\":2", "\"runtime_protocol\":\"4\""),
            ("\"memory_dex\":false", "\"memory_dex\":falsehood"),
            (
                "\"environment_policy\":false",
                "\"environment_policy\":false,\"environment_policy\":true",
            ),
            (
                "\"memory_dex\":false",
                "\"memory_dex\":false,\"min_android_api\":19,\"min_android_api\":21",
            ),
        ] {
            assert!(RuntimeMetadata::parse(&METADATA.replace(from, to)).is_err());
        }
    }

    #[test]
    fn 最低系统声明区分兼容资源且不接受无效值() {
        let api19 = METADATA.replace(
            "\"memory_dex\":false",
            "\"memory_dex\":false,\"min_android_api\":19",
        );
        assert_eq!(RuntimeMetadata::parse(&api19).unwrap().min_android_api, 19);
        let invalid = api19.replace("\"min_android_api\":19", "\"min_android_api\":\"19\"");
        assert!(RuntimeMetadata::parse(&invalid).is_err());
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
                "\"memory_dex\":true,\n        \"memory_dex_min_api\":31,\n        \"stub_component_factory\":\"msk.f\"",
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
                "\"memory_dex\":true,\n        \"memory_dex_min_api\":31,\n        \"stub_component_factory\":\"invalid factory\"",
            );
        assert!(RuntimeMetadata::parse(&candidate).is_err());
    }
}

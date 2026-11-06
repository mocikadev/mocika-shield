//! JSON 文档边界只处理语法、根级重复与字段来源，协议语义仍由 RuntimeMetadata 解释。
use crate::protect::runtime_metadata::RuntimeMetadata;
use anyhow::{ensure, Context, Result};
use serde::de::{Error, MapAccess, Visitor};
use serde::{Deserialize, Deserializer};
use serde_json::{Map, Value};
use std::fmt;

// 传给旧标量解析器的已知根字段，避免未知嵌套字段被误当成协议声明。
const NUMBERS: &[&str] = &[
    "runtime_protocol",
    "cache_schema",
    "min_android_api",
    "memory_dex_min_api",
    "native_name_length",
    "native_name_scheme",
];
const BOOLEANS: &[&str] = &["environment_policy", "memory_dex"];
const STRINGS: &[&str] = &[
    "stub_application",
    "stub_component_factory",
    "native_library",
    "native_name_placeholder",
];

struct Root(Map<String, Value>);

impl<'de> Deserialize<'de> for Root {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> std::result::Result<Self, D::Error> {
        struct RootVisitor;
        impl<'de> Visitor<'de> for RootVisitor {
            type Value = Root;
            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("根级唯一字段的资源元数据对象")
            }
            fn visit_map<M: MapAccess<'de>>(
                self,
                mut access: M,
            ) -> std::result::Result<Root, M::Error> {
                let mut values = Map::new();
                while let Some((key, value)) = access.next_entry::<String, Value>()? {
                    if values.insert(key, value).is_some() {
                        return Err(M::Error::custom("资源元数据存在重复根字段"));
                    }
                }
                Ok(Root(values))
            }
        }
        deserializer.deserialize_map(RootVisitor)
    }
}

pub(super) fn parse(content: &str) -> Result<RuntimeMetadata> {
    let mut root: Root = serde_json::from_str(content).context("资源元数据 JSON 文档无效")?;
    for (name, value) in &root.0 {
        if NUMBERS.contains(&name.as_str()) {
            ensure!(
                value.as_u64().is_some_and(|n| u32::try_from(n).is_ok()),
                "资源整数声明类型无效"
            );
        } else if BOOLEANS.contains(&name.as_str()) {
            ensure!(value.is_boolean(), "资源布尔声明类型无效");
        } else if STRINGS.contains(&name.as_str()) {
            ensure!(value.is_string(), "资源字符串声明类型无效");
        }
    }
    root.0.retain(|name, _| {
        NUMBERS.contains(&name.as_str())
            || BOOLEANS.contains(&name.as_str())
            || STRINGS.contains(&name.as_str())
    });
    let canonical = serde_json::to_string(&root.0).context("规范化资源元数据失败")?;
    let checked = RuntimeMetadata::parse(&canonical)?;
    // 新入口不能因 JSON 规范化比旧执行链更宽松，也不能让嵌套声明改变旧解析结果。
    let actual = RuntimeMetadata::parse(content)?;
    ensure!(
        checked.stub_application == actual.stub_application
            && checked.stub_component_factory == actual.stub_component_factory
            && checked.native_alias == actual.native_alias
            && checked.environment_policy == actual.environment_policy
            && checked.memory_dex == actual.memory_dex
            && checked.min_android_api == actual.min_android_api
            && std::ptr::eq(checked.manifest_keys, actual.manifest_keys),
        "资源文档与执行链的协议解释不一致"
    );
    Ok(actual)
}

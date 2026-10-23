// 由 scripts/manifest_contract.py 生成，请修改单一契约定义。

#[derive(Debug)]
pub(crate) struct ManifestKeys {
    pub(crate) application: &'static str,
    pub(crate) policy: &'static str,
    pub(crate) factory: &'static str,
    pub(crate) cache_schema: &'static str,
    pub(crate) cache_count: &'static str,
    pub(crate) cache_root: &'static str,
    pub(crate) payload_bytes: &'static str,
}

pub(crate) const LEGACY: ManifestKeys = ManifestKeys {
    application: "ORIGINAL_APPLICATION",
    policy: "dev.mocika.shield.ENV_POLICY",
    factory: "dev.mocika.shield.ORIGINAL_COMPONENT_FACTORY",
    cache_schema: "dev.mocika.shield.CACHE_SCHEMA",
    cache_count: "dev.mocika.shield.CACHE_DEX_COUNT",
    cache_root: "dev.mocika.shield.CACHE_ROOT_SHA256",
    payload_bytes: "dev.mocika.shield.PAYLOAD_DEX_BYTES",
};

pub(crate) const CURRENT: ManifestKeys = ManifestKeys {
    application: "rt.app",
    policy: "rt.policy",
    factory: "rt.factory",
    cache_schema: "rt.cache.schema",
    cache_count: "rt.cache.count",
    cache_root: "rt.cache.root",
    payload_bytes: "rt.payload.bytes",
};

pub(crate) const LEGACY_STANDARD_PROTOCOL: u32 = 2;
pub(crate) const LEGACY_MEMORY_PROTOCOL: u32 = 3;
pub(crate) const STANDARD_PROTOCOL: u32 = 4;
pub(crate) const MEMORY_PROTOCOL: u32 = 5;

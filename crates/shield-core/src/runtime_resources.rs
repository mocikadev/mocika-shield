//! 只读资源准入：不公开内部资源模型，不改变既有加固或预检入口。
mod archive;
mod document;

use crate::protect::abi_filter::STANDARD_ABIS;
use crate::{EnvironmentPolicy, RuntimeProfile};
use anyhow::{ensure, Context, Result};
use std::collections::HashSet;
use std::fs::File;
use std::io::Read;
use std::path::Path;

const MAX_METADATA_BYTES: u64 = 64 * 1024;

/// 只读核对资源协议、请求模式、环境策略、Stub 与所需规范壳库及归档完整性。
///
/// `required_abis` 是原 APK 明确排除旧架构后的保留集合；空集合表示纯 DEX APK，
/// 与现有注入规则一致，须提供四个主流壳架构。工控模式有 Native 库时须保留 v7a。
/// 不验证 ELF/DEX 全面语义、资源来源或设备运行；不执行 Java 或解包到磁盘。
pub fn validate_runtime_resources(
    resources_path: &Path,
    expected_profile: RuntimeProfile,
    environment_policy: EnvironmentPolicy,
    required_abis: &[String],
) -> Result<()> {
    ensure!(
        required_abis.len() <= STANDARD_ABIS.len(),
        "所需资源 ABI 数量无效"
    );
    let unique: HashSet<_> = required_abis.iter().collect();
    ensure!(unique.len() == required_abis.len(), "所需资源 ABI 不能重复");
    expected_profile.validate_native_abis(required_abis, &[])?;
    let mut input = File::open(resources_path).context("打开资源包失败")?;
    archive::check_bounds(&mut input)?;
    let mut raw = input.try_clone().context("读取资源归档结构失败")?;
    let mut archive = zip::ZipArchive::new(input).context("解析资源归档失败")?;
    let entries = archive::verify(&mut archive, &mut raw)?;
    let content = {
        let mut entry = archive
            .by_name("metadata.json")
            .context("资源缺少 metadata.json")?;
        ensure!(entry.size() <= MAX_METADATA_BYTES, "资源元数据超出大小限制");
        let mut content = String::new();
        (&mut entry)
            .take(MAX_METADATA_BYTES + 1)
            .read_to_string(&mut content)
            .context("资源元数据不是有效 UTF-8")?;
        ensure!(
            content.len() as u64 <= MAX_METADATA_BYTES,
            "资源元数据超出大小限制"
        );
        content
    };
    let metadata = document::parse(&content)?;
    metadata.validate_policy(environment_policy, false)?;
    let actual = match metadata.min_android_api {
        19 => RuntimeProfile::AndroidApi19,
        21 => RuntimeProfile::Standard,
        _ => anyhow::bail!("资源声明了无法确认的最低系统 API"),
    };
    ensure!(actual == expected_profile, "资源运行模式与请求模式不匹配");
    for name in entries.keys() {
        let allowed = matches!(name.as_str(), "metadata.json" | "stub-classes.dex" | "lib/")
            || STANDARD_ABIS.iter().any(|abi| {
                name == &format!("lib/{abi}/")
                    || name == &format!("lib/{abi}/{}", metadata.native_alias.canonical_file_name)
            });
        ensure!(allowed, "资源包含非规范条目，不能用于自动化计划");
    }
    ensure!(
        entries
            .get("stub-classes.dex")
            .is_some_and(|size| *size > 0),
        "资源缺少非空 Stub DEX"
    );
    let abis: Vec<&str> = if required_abis.is_empty() {
        STANDARD_ABIS.to_vec()
    } else {
        required_abis.iter().map(String::as_str).collect()
    };
    for abi in abis {
        let name = format!("lib/{abi}/{}", metadata.native_alias.canonical_file_name);
        ensure!(
            entries.get(&name).is_some_and(|size| *size > 0),
            "资源缺少所需的非空规范壳库"
        );
    }
    Ok(())
}

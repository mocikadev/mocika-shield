//! CLI 只选择本地资源并映射安全错误；资源协议和壳清单只由核心解释。
use super::{require_file, CheckError};
use shield_core::utils::find_runtime_resources;
use shield_core::{EnvironmentPolicy, RuntimeProfile};
use std::path::{Path, PathBuf};

pub(super) fn select(
    explicit: Option<PathBuf>,
    profile: RuntimeProfile,
) -> Result<PathBuf, CheckError> {
    select_from(explicit, profile, find_runtime_resources)
}

fn select_from(
    explicit: Option<PathBuf>,
    profile: RuntimeProfile,
    find_standard: impl FnOnce() -> anyhow::Result<PathBuf>,
) -> Result<PathBuf, CheckError> {
    let path = match explicit {
        Some(path) => path,
        None => {
            let standard = find_standard().map_err(|_| unavailable())?;
            match profile {
                RuntimeProfile::Standard => standard,
                RuntimeProfile::AndroidApi19 => standard.with_file_name("resources-api19.zip"),
            }
        }
    };
    require_file(&path, "RUNTIME_RESOURCE_UNAVAILABLE").map_err(|_| unavailable())?;
    Ok(path)
}

pub(super) fn validate(
    path: &Path,
    profile: RuntimeProfile,
    policy: EnvironmentPolicy,
    original_abis: &[String],
    excluded: &[String],
) -> Result<(), CheckError> {
    let retained: Vec<_> = original_abis
        .iter()
        .filter(|abi| !excluded.contains(abi))
        .cloned()
        .collect();
    shield_core::validate_runtime_resources(path, profile, policy, &retained).map_err(|_| {
        CheckError::new(
            "RUNTIME_RESOURCE_INVALID",
            "资源协议、运行模式、安全策略或所需壳条目不符合本次计划，不会自动替换或降级。",
        )
    })
}

fn unavailable() -> CheckError {
    CheckError::new(
        "RUNTIME_RESOURCE_UNAVAILABLE",
        "所选模式的资源包缺失或不可读，请在本机检查完整资源，不会回退其他模式。",
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn 缺工控包不能回退标准资源() {
        let dir = tempfile::tempdir().unwrap();
        let standard = dir.path().join("resources.zip");
        fs::write(&standard, "仅验证定位").unwrap();
        let error = select_from(None, RuntimeProfile::AndroidApi19, || Ok(standard.clone()))
            .err()
            .unwrap();
        assert_eq!(error.code, "RUNTIME_RESOURCE_UNAVAILABLE");
        let compat = dir.path().join("resources-api19.zip");
        fs::write(&compat, b"fixture").unwrap();
        assert_eq!(
            select_from(None, RuntimeProfile::AndroidApi19, || Ok(standard))
                .ok()
                .unwrap(),
            compat
        );
    }

    #[test]
    fn 显式覆盖不忽略也不自动替换() {
        let dir = tempfile::tempdir().unwrap();
        let custom = dir.path().join("custom.zip");
        fs::write(&custom, b"fixture").unwrap();
        assert_eq!(
            select_from(
                Some(custom.clone()),
                RuntimeProfile::AndroidApi19,
                || panic!("不应定位默认资源")
            )
            .ok()
            .unwrap(),
            custom
        );
        assert!(select_from(
            Some(dir.path().join("missing.zip")),
            RuntimeProfile::Standard,
            || panic!("不能回退")
        )
        .is_err());
    }
}

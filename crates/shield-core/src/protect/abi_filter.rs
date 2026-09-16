use anyhow::{bail, Result};
use std::{fs, path::Path};

pub(crate) const STANDARD_ABIS: &[&str] = &["armeabi-v7a", "arm64-v8a", "x86", "x86_64"];

pub(crate) fn validate_exclusions(original: &[String], excluded: &[String]) -> Result<()> {
    if excluded
        .iter()
        .collect::<std::collections::HashSet<_>>()
        .len()
        != excluded.len()
    {
        bail!("ABI 排除清单存在重复项");
    }
    for abi in excluded {
        if abi.is_empty()
            || !abi
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
            || STANDARD_ABIS.contains(&abi.as_str())
            || !original.contains(abi)
        {
            bail!("不能排除此 ABI：{abi}；仅允许排除 APK 中实际存在且不受支持的架构");
        }
    }
    if !original.is_empty()
        && !original
            .iter()
            .any(|abi| STANDARD_ABIS.contains(&abi.as_str()))
    {
        bail!("APK 没有受支持的 Native 架构，不能通过排除架构继续加固");
    }
    let unsupported: Vec<_> = original
        .iter()
        .filter(|abi| !STANDARD_ABIS.contains(&abi.as_str()) && !excluded.contains(abi))
        .collect();
    if !unsupported.is_empty() {
        bail!(
            "APK 包含不受支持的 ABI：{}；请在原工程排除，或显式确认 --exclude-abis 后重试",
            unsupported
                .iter()
                .map(|s| s.as_str())
                .collect::<Vec<_>>()
                .join("、")
        );
    }
    Ok(())
}

pub(crate) fn remove_excluded(apk_dir: &Path, excluded: &[String]) -> Result<()> {
    let lib = apk_dir.join("lib");
    if excluded.is_empty() {
        return Ok(());
    }
    if fs::symlink_metadata(&lib)?.file_type().is_symlink() {
        bail!("拒绝清理符号链接 lib 目录");
    }
    for abi in excluded {
        if abi.is_empty()
            || !abi
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
            || STANDARD_ABIS.contains(&abi.as_str())
        {
            bail!("非法 ABI 清理目标：{abi}");
        }
        let path = lib.join(abi);
        let metadata = fs::symlink_metadata(&path)?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            bail!("ABI 清理目标不是普通目录：{abi}");
        }
        fs::remove_dir_all(path)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    #[test]
    fn 混合架构仅在显式排除后通过() {
        let original = names(&["arm64-v8a", "mips", "armeabi"]);
        assert!(validate_exclusions(&original, &[]).is_err());
        assert!(validate_exclusions(&original, &names(&["mips"])).is_err());
        assert!(validate_exclusions(&original, &names(&["mips", "armeabi"])).is_ok());
    }

    #[test]
    fn 拒绝无支持架构及非法清单() {
        assert!(
            validate_exclusions(&names(&["arm64-v8a", "mips"]), &names(&["mips", "mips"])).is_err()
        );
        assert!(validate_exclusions(&names(&["mips"]), &names(&["mips"])).is_err());
        for excluded in ["arm64-v8a", "../assets", "mips64"] {
            assert!(
                validate_exclusions(&names(&["arm64-v8a", "mips"]), &names(&[excluded])).is_err()
            );
        }
        assert!(validate_exclusions(&[], &[]).is_ok());
        assert!(validate_exclusions(&names(STANDARD_ABIS), &[]).is_ok());
    }

    #[test]
    fn 只清理明确排除的临时架构目录() {
        let temp = tempfile::tempdir().unwrap();
        for abi in ["mips", "arm64-v8a"] {
            fs::create_dir_all(temp.path().join("lib").join(abi)).unwrap();
            fs::write(
                temp.path().join("lib").join(abi).join("libtest.so"),
                b"original",
            )
            .unwrap();
        }
        remove_excluded(temp.path(), &names(&["mips"])).unwrap();
        assert!(!temp.path().join("lib/mips").exists());
        assert_eq!(
            fs::read(temp.path().join("lib/arm64-v8a/libtest.so")).unwrap(),
            b"original"
        );
    }

    #[cfg(unix)]
    #[test]
    fn 拒绝链接及路径逃逸且外部文件不变() {
        let temp = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        fs::write(outside.path().join("keep"), b"keep").unwrap();
        fs::create_dir(temp.path().join("lib")).unwrap();
        std::os::unix::fs::symlink(outside.path(), temp.path().join("lib/mips")).unwrap();
        assert!(remove_excluded(temp.path(), &names(&["mips"])).is_err());
        assert!(remove_excluded(temp.path(), &names(&["../outside"])).is_err());
        assert_eq!(fs::read(outside.path().join("keep")).unwrap(), b"keep");
    }
}

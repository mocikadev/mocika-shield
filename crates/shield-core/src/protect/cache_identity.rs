use anyhow::{Context, Result};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};

pub(crate) const CACHE_SCHEMA: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CacheIdentity {
    pub(crate) schema: u32,
    pub(crate) dex_count: usize,
    pub(crate) total_dex_bytes: u64,
    pub(crate) root_sha256: String,
}

pub(crate) fn calculate(dex_files: &[PathBuf]) -> Result<CacheIdentity> {
    if dex_files.is_empty() {
        anyhow::bail!("无法为空 DEX 集合生成缓存身份");
    }
    let mut files = dex_files.to_vec();
    files.sort_by_key(|path| dex_index(path));

    let mut root = Sha256::new();
    let mut total_dex_bytes = 0u64;
    root.update(CACHE_SCHEMA.to_le_bytes());
    root.update((files.len() as u32).to_le_bytes());
    for (position, path) in files.iter().enumerate() {
        let name = path
            .file_name()
            .and_then(|value| value.to_str())
            .context("DEX 文件名不是有效 UTF-8")?;
        let data = fs::read(path).with_context(|| format!("读取 DEX 生成缓存身份失败: {name}"))?;
        let digest = Sha256::digest(&data);
        total_dex_bytes = total_dex_bytes
            .checked_add(data.len() as u64)
            .context("DEX 总大小溢出")?;
        root.update(((position + 1) as u32).to_le_bytes());
        root.update((name.len() as u32).to_le_bytes());
        root.update(name.as_bytes());
        root.update((data.len() as u64).to_le_bytes());
        root.update(digest);
    }

    Ok(CacheIdentity {
        schema: CACHE_SCHEMA,
        dex_count: files.len(),
        total_dex_bytes,
        root_sha256: hex_lower(&root.finalize()),
    })
}

fn dex_index(path: &Path) -> u32 {
    let name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("");
    if name == "classes.dex" {
        return 1;
    }
    name.strip_prefix("classes")
        .and_then(|value| value.strip_suffix(".dex"))
        .and_then(|value| value.parse().ok())
        .unwrap_or(u32::MAX)
}

fn hex_lower(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use std::fmt::Write as _;
        let _ = write!(output, "{byte:02x}");
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_is_stable_regardless_of_input_order() {
        let dir = tempfile::tempdir().unwrap();
        let first = dir.path().join("classes.dex");
        let second = dir.path().join("classes2.dex");
        fs::write(&first, b"first").unwrap();
        fs::write(&second, b"second").unwrap();
        let forward = calculate(&[first.clone(), second.clone()]).unwrap();
        let reverse = calculate(&[second, first]).unwrap();
        assert_eq!(forward, reverse);
        assert_eq!(forward.dex_count, 2);
        assert_eq!(forward.total_dex_bytes, 11);
        assert_eq!(forward.root_sha256.len(), 64);
    }

    #[test]
    fn identity_changes_when_dex_content_changes() {
        let dir = tempfile::tempdir().unwrap();
        let dex = dir.path().join("classes.dex");
        fs::write(&dex, b"before").unwrap();
        let before = calculate(std::slice::from_ref(&dex)).unwrap();
        fs::write(&dex, b"after").unwrap();
        let after = calculate(&[dex]).unwrap();
        assert_ne!(before.root_sha256, after.root_sha256);
    }

    #[test]
    fn empty_collection_is_rejected() {
        assert!(calculate(&[]).is_err());
    }
}

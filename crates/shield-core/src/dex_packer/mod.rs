mod crypto;
mod packer;
pub mod route_scanner;

use anyhow::{Context, Result};
use std::fs;
use std::path::Path;

pub fn pack_dex_files(
    input_dir: &Path,
    output_bin: &Path,
    ikm: &[u8],
    signature: &str,
) -> Result<()> {
    if !input_dir.is_dir() {
        anyhow::bail!("输入目录不存在: {:?}", input_dir);
    }

    let mut dex_files = collect_dex_files(input_dir)?;

    if dex_files.is_empty() {
        anyhow::bail!("目录中没有找到.dex文件");
    }

    dex_files.sort_by(|a, b| {
        let name_a = a.file_name().unwrap_or_default().to_string_lossy();
        let name_b = b.file_name().unwrap_or_default().to_string_lossy();
        dex_natural_key(&name_a).cmp(&dex_natural_key(&name_b))
    });

    println!("✓ 找到 {} 个DEX文件", dex_files.len());

    let mut packer = packer::DexPacker::new();

    for dex_path in &dex_files {
        let name = dex_path.file_name().unwrap().to_string_lossy().to_string();
        packer.add_dex(dex_path, &name)?;
    }

    packer.pack(output_bin, ikm, signature)?;

    Ok(())
}

fn collect_dex_files(dir: &Path) -> Result<Vec<std::path::PathBuf>> {
    let mut dex_files = Vec::new();

    for entry in fs::read_dir(dir).context("读取目录失败")? {
        let entry = entry.context("读取目录项失败")?;
        let path = entry.path();

        if path.is_file() && path.extension().and_then(|s| s.to_str()) == Some("dex") {
            dex_files.push(path);
        }
    }

    Ok(dex_files)
}

fn dex_natural_key(name: &str) -> u32 {
    if name == "classes.dex" {
        return 1;
    }
    name.trim_start_matches("classes")
        .trim_end_matches(".dex")
        .parse::<u32>()
        .unwrap_or(u32::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn natural_sort_classes_dex_first() {
        let mut names = vec![
            "classes3.dex".to_string(),
            "classes.dex".to_string(),
            "classes2.dex".to_string(),
        ];
        names.sort_by_key(|a| dex_natural_key(a));
        assert_eq!(names, vec!["classes.dex", "classes2.dex", "classes3.dex"]);
    }

    #[test]
    fn natural_sort_two_digit_index_not_sorted_before_single() {
        let mut names = vec![
            "classes10.dex".to_string(),
            "classes2.dex".to_string(),
            "classes.dex".to_string(),
        ];
        names.sort_by_key(|a| dex_natural_key(a));
        assert_eq!(names, vec!["classes.dex", "classes2.dex", "classes10.dex"]);
    }
}

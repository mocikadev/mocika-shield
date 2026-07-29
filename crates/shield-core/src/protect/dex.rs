use anyhow::{Context, Result};
use std::fs;
use std::path::Path;

use crate::protect::cache_identity::{self, CacheIdentity};
use crate::utils::{human_size, print_success};

pub(crate) fn process_dex(apk_dir: &Path, signature: &str, ikm: &[u8]) -> Result<CacheIdentity> {
    let mut dex_files = Vec::new();
    for entry in fs::read_dir(apk_dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) == Some("dex") {
            dex_files.push(path);
        }
    }

    if dex_files.is_empty() {
        anyhow::bail!("未找到DEX文件");
    }
    let cache_identity = cache_identity::calculate(&dex_files)?;

    let dex_dir = apk_dir.parent().unwrap().join("dex");
    fs::create_dir_all(&dex_dir)?;

    for dex_file in &dex_files {
        let dest = dex_dir.join(dex_file.file_name().unwrap());
        fs::rename(dex_file, dest)?;
    }

    let assets_dir = apk_dir.join("assets");
    fs::create_dir_all(&assets_dir)?;
    let routes = crate::dex_packer::route_scanner::scan_arouter_routes(&dex_dir)?;
    if !routes.is_empty() {
        let route_file = assets_dir.join("arouter_routes.txt");
        fs::write(&route_file, routes.join("\n"))?;
        print_success(&format!("扫描到 ARouter 路由表类 {} 个", routes.len()));
    }

    let tmp_bin = apk_dir.parent().unwrap().join("app.bin.tmp");
    crate::dex_packer::pack_dex_files(&dex_dir, &tmp_bin, ikm, signature)?;

    let bin_size = fs::metadata(&tmp_bin)?.len();
    print_success(&format!("DEX打包完成: {}", human_size(bin_size)));

    Ok(cache_identity)
}

pub(crate) fn patch_dex_header(dex_path: &Path) -> Result<()> {
    use sha1::{Digest, Sha1};
    use std::io::{Read, Seek, SeekFrom, Write};

    let mut file = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(dex_path)
        .context("打开 classes.dex 读写失败")?;

    let mut data = Vec::new();
    file.read_to_end(&mut data)
        .context("读取 classes.dex 全量失败")?;

    if data.len() < 36 {
        anyhow::bail!("classes.dex 过短，无法修复 header（{}字节）", data.len());
    }

    let total_len =
        u32::try_from(data.len()).context("classes.dex 超过 4GiB，无法写入 file_size")?;

    data[32..36].copy_from_slice(&total_len.to_le_bytes());

    let sig = Sha1::digest(&data[32..]);
    data[12..32].copy_from_slice(&sig);

    let checksum = adler32_checksum(&data[12..]);
    data[8..12].copy_from_slice(&checksum.to_le_bytes());

    file.seek(SeekFrom::Start(0))
        .context("定位 classes.dex 写入位置失败")?;
    file.write_all(&data)
        .context("回写 classes.dex header 失败")?;

    Ok(())
}

fn adler32_checksum(data: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    for &byte in data {
        a = (a + byte as u32) % 65521;
        b = (b + a) % 65521;
    }
    (b << 16) | a
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adler32_empty_input_returns_one() {
        assert_eq!(adler32_checksum(b""), 1);
    }

    #[test]
    fn adler32_wikipedia_known_value() {
        assert_eq!(adler32_checksum(b"Wikipedia"), 0x11E60398);
    }
}

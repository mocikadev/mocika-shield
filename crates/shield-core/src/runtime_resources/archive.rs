//! 资源归档的有界只读清点，不复用为 APK 解包器。
use anyhow::{ensure, Context, Result};
use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};

const MAX_ENTRIES: usize = 256;
const MAX_CONTENT_BYTES: u64 = 256 * 1024 * 1024;

pub(super) fn check_bounds(raw: &mut File) -> Result<()> {
    let size = raw.metadata()?.len();
    ensure!(
        (22..=MAX_CONTENT_BYTES).contains(&size),
        "资源归档文件大小不在支持范围"
    );
    raw.seek(SeekFrom::End(-22))?;
    let mut footer = [0u8; 22];
    raw.read_exact(&mut footer)?;
    ensure!(
        &footer[..4] == b"PK\x05\x06" && footer[20..22] == [0, 0],
        "资源只读准入仅支持无注释的一致 ZIP32 归档"
    );
    let count = u16::from_le_bytes([footer[10], footer[11]]);
    ensure!(usize::from(count) <= MAX_ENTRIES, "资源条目数量超限");
    Ok(())
}

pub(super) fn verify(
    archive: &mut zip::ZipArchive<File>,
    raw: &mut File,
) -> Result<HashMap<String, u64>> {
    verify_directory(archive, raw)?;
    ensure!(archive.len() <= MAX_ENTRIES, "资源条目数量超限");
    let mut seen = HashMap::<String, bool>::new();
    let mut files = HashMap::new();
    let mut total = 0u64;
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).context("读取资源条目失败")?;
        let name = std::str::from_utf8(entry.name_raw())
            .context("资源条目名称不是 UTF-8")?
            .to_owned();
        validate_name(&name, entry.is_dir())?;
        let kind = entry.unix_mode().unwrap_or(0) & 0o170000;
        ensure!(
            matches!(kind, 0 | 0o100000 | 0o040000),
            "资源不允许符号链接或特殊文件"
        );
        ensure!(
            kind == 0 || (kind == 0o040000) == entry.is_dir(),
            "资源文件类型与路径不一致"
        );
        let key = name.trim_end_matches('/').to_ascii_lowercase();
        ensure!(
            seen.insert(key, entry.is_dir()).is_none(),
            "资源存在重复或冲突路径"
        );
        total = total
            .checked_add(entry.size())
            .context("资源条目大小溢出")?;
        ensure!(total <= MAX_CONTENT_BYTES, "资源解压内容大小超限");
        if name == "metadata.json" {
            ensure!(
                entry.size() <= super::MAX_METADATA_BYTES,
                "资源元数据超出大小限制"
            );
        }
        if entry.is_dir() {
            ensure!(entry.size() == 0, "资源目录条目不应包含内容");
            files.insert(name, 0);
        } else {
            let expected = entry.size();
            // 多读一字节以识别长度不一致，同时限制畸形压缩流的展开量。
            let count = std::io::copy(&mut (&mut entry).take(expected + 1), &mut std::io::sink())
                .context("资源条目长度、压缩流或 CRC 校验失败")?;
            ensure!(count == expected, "资源条目实际长度与声明不一致");
            files.insert(name, expected);
        }
    }
    for name in seen.keys() {
        let mut parent = name.as_str();
        while let Some((prefix, _)) = parent.rsplit_once('/') {
            ensure!(
                seen.get(prefix) != Some(&false),
                "资源文件不能同时作为其他条目的父目录"
            );
            parent = prefix;
        }
    }
    Ok(files)
}

fn verify_directory(archive: &zip::ZipArchive<File>, raw: &mut File) -> Result<()> {
    let size = raw.metadata()?.len();
    ensure!(size <= MAX_CONTENT_BYTES, "资源归档文件大小超限");
    let end = size
        .checked_sub(22 + archive.comment().len() as u64)
        .context("资源归档尾部不完整")?;
    raw.seek(SeekFrom::Start(end))?;
    let mut footer = [0u8; 22];
    raw.read_exact(&mut footer)?;
    let word = |offset| u16::from_le_bytes([footer[offset], footer[offset + 1]]);
    let dword = |offset| u32::from_le_bytes(footer[offset..offset + 4].try_into().unwrap());
    ensure!(&footer[..4] == b"PK\x05\x06", "资源归档尾部签名无效");
    ensure!(
        archive.offset() == 0 && word(4) == 0 && word(6) == 0,
        "资源归档不支持前缀或分卷布局"
    );
    // zip 的名称索引会去重；必须与原始目录计数比较，不能用去重后的条目证明无重复。
    ensure!(
        word(8) == word(10) && usize::from(word(10)) == archive.len(),
        "资源目录计数不一致或存在重复条目"
    );
    ensure!(
        u64::from(dword(16)) == archive.central_directory_start()
            && u64::from(dword(16)) + u64::from(dword(12)) == end,
        "资源归档不是支持的一致 ZIP32 布局"
    );
    Ok(())
}

fn validate_name(name: &str, directory: bool) -> Result<()> {
    ensure!(name.len() <= 1024, "资源条目名称超限");
    ensure!(
        !name.is_empty() && !name.starts_with('/') && !name.contains('\\'),
        "资源路径不是规范相对路径"
    );
    let normalized = if directory {
        name.strip_suffix('/').unwrap_or(name)
    } else {
        name
    };
    for part in normalized.split('/') {
        ensure!(
            !part.is_empty() && part != "." && part != "..",
            "资源路径包含空段或跨目录段"
        );
        ensure!(
            !part.ends_with(['.', ' '])
                && !part
                    .chars()
                    .any(|c| c.is_control() || ":*?\"<>|".contains(c)),
            "资源路径不能跨平台安全使用"
        );
        let stem = part.split('.').next().unwrap_or("").to_ascii_uppercase();
        let device = matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
            || stem
                .strip_prefix("COM")
                .or_else(|| stem.strip_prefix("LPT"))
                .is_some_and(|suffix| {
                    suffix.len() == 1 && matches!(suffix.as_bytes()[0], b'1'..=b'9')
                });
        ensure!(!device, "资源路径包含保留设备名称");
    }
    Ok(())
}

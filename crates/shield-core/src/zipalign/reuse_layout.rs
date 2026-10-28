//! 快路径的保守资格检查；只读取 ZIP32 头部，不改写布局或替代 ZIP 解码器。

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use anyhow::Result;
use zip::{CompressionMethod, ZipArchive};

fn word(bytes: &[u8], start: usize) -> u16 {
    u16::from_le_bytes(bytes[start..start + 2].try_into().unwrap())
}

fn dword(bytes: &[u8], start: usize) -> u32 {
    u32::from_le_bytes(bytes[start..start + 4].try_into().unwrap())
}

fn read_at<const N: usize>(input: &mut File, offset: u64) -> std::io::Result<[u8; N]> {
    input.seek(SeekFrom::Start(offset))?;
    let mut bytes = [0; N];
    input.read_exact(&mut bytes)?;
    Ok(bytes)
}

pub(super) fn is_simple_layout(path: &Path, archive: &mut ZipArchive<File>) -> Result<bool> {
    let mut input = File::open(path)?;
    let length = input.metadata()?.len();
    let Some(end) = length.checked_sub(22 + archive.comment().len() as u64) else {
        return Ok(false);
    };
    let footer = read_at::<22>(&mut input, end)?;
    // 对前缀、多磁盘、ZIP64、额外尾部等布局不做推测，保留旧重写路径。
    if &footer[..4] != b"PK\x05\x06"
        || archive.offset() != 0
        || word(&footer, 4) != 0
        || word(&footer, 6) != 0
        || word(&footer, 8) != word(&footer, 10)
        || word(&footer, 10) == u16::MAX
        || usize::from(word(&footer, 10)) != archive.len()
        || usize::from(word(&footer, 20)) != archive.comment().len()
        || u64::from(dword(&footer, 16)) != archive.central_directory_start()
        || u64::from(dword(&footer, 16)) + u64::from(dword(&footer, 12)) != end
    {
        return Ok(false);
    }

    let mut local_end = 0;
    let mut central_end = archive.central_directory_start();
    for index in 0..archive.len() {
        let file = archive.by_index(index)?;
        if file.header_start() != local_end || file.central_header_start() != central_end {
            return Ok(false);
        }
        let Some((next_local, next_central)) = entry_ends(&mut input, &file)? else {
            return Ok(false);
        };
        local_end = next_local;
        central_end = next_central;
    }
    // 不原样保留隐藏本地条目、重复中央记录、签名块或无法解释的间隙。
    Ok(local_end == archive.central_directory_start() && central_end == end)
}

fn entry_ends(input: &mut File, file: &zip::read::ZipFile<'_>) -> Result<Option<(u64, u64)>> {
    let central = read_at::<46>(input, file.central_header_start())?;
    if &central[..4] != b"PK\x01\x02" || word(&central, 34) != 0 {
        return Ok(None);
    }
    let central_end = file.central_header_start()
        + 46
        + u64::from(word(&central, 28))
        + u64::from(word(&central, 30))
        + u64::from(word(&central, 32));
    let local = read_at::<30>(input, file.header_start())?;
    let method = match file.compression() {
        CompressionMethod::Stored => 0,
        CompressionMethod::Deflated => 8,
        _ => return Ok(None),
    };
    let flags = word(&local, 6);
    let name_length = word(&local, 26);
    let data_start =
        file.header_start() + 30 + u64::from(name_length) + u64::from(word(&local, 28));
    if &local[..4] != b"PK\x03\x04"
        || !matches!(word(&local, 4), 10 | 20)
        || word(&central, 6) != word(&local, 4)
        || word(&central, 8) != flags
        || flags & !0x0806 != 0
        || word(&local, 8) != method
        || local[10..14] != central[12..16]
        || dword(&local, 14) != file.crc32()
        || u64::from(dword(&local, 18)) != file.compressed_size()
        || u64::from(dword(&local, 22)) != file.size()
        || data_start != file.data_start()
    {
        return Ok(None);
    }
    let mut name = vec![0; usize::from(name_length)];
    input.read_exact(&mut name)?;
    if name != file.name_raw()
        || (!name.is_ascii() && flags & 0x0800 == 0)
        || std::str::from_utf8(&name).ok() != Some(file.name())
        || (file.is_dir()
            && (file.compression() != CompressionMethod::Stored
                || file.size() != 0
                || file.compressed_size() != 0
                || file.crc32() != 0))
    {
        return Ok(None);
    }
    // 本地与中央附加字段可不同，但各自必须完整落在声明的有界区间。
    if !has_simple_extra_fields(
        input,
        file.header_start() + 30 + u64::from(name_length),
        word(&local, 28),
    )? || !has_simple_extra_fields(
        input,
        file.central_header_start() + 46 + u64::from(word(&central, 28)),
        word(&central, 30),
    )? {
        return Ok(None);
    }
    Ok(Some((data_start + file.compressed_size(), central_end)))
}

fn has_simple_extra_fields(input: &mut File, offset: u64, length: u16) -> Result<bool> {
    input.seek(SeekFrom::Start(offset))?;
    // ZIP32 单处附加区长度为 u16，读取和内存占用均有上界。
    let mut bytes = vec![0; usize::from(length)];
    input.read_exact(&mut bytes)?;
    let mut remaining = bytes.as_slice();
    while !remaining.is_empty() {
        // 不解释未知字段语义；ZIP64 即便尺寸看似 ZIP32 也不在复用范围。
        if remaining.len() < 4 || word(remaining, 0) == 0x0001 {
            return Ok(false);
        }
        let field_end = 4 + usize::from(word(remaining, 2));
        let Some(tail) = remaining.get(field_end..) else {
            return Ok(false);
        };
        remaining = tail;
    }
    Ok(true)
}

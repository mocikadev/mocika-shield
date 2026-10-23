//! 严格读取 DEX 类定义，用于路由发现与壳类冲突检查，不修改字节码。

use anyhow::{bail, Context, Result};
use std::collections::HashSet;

pub(crate) fn defined_classes(data: &[u8]) -> Result<HashSet<String>> {
    if data.len() < 112
        || !matches!(
            &data[..8],
            b"dex\n035\0" | b"dex\n037\0" | b"dex\n038\0" | b"dex\n039\0" | b"dex\n040\0"
        )
    {
        bail!("不支持或不完整的 DEX 格式");
    }
    let size = u32_at(data, 32)? as usize;
    if !(112..=data.len()).contains(&size)
        || u32_at(data, 36)? != 112
        || u32_at(data, 40)? != 0x12345678
    {
        bail!("DEX 头长度或字节序无效");
    }
    let data = &data[..size];
    let strings = table(data, 56, 4)?;
    let types = table(data, 64, 4)?;
    let classes = table(data, 96, 32)?;
    let mut names = HashSet::new();
    for offset in classes {
        let type_index = u32_at(data, offset)? as usize;
        let type_offset = *types.get(type_index).context("DEX 类类型索引越界")?;
        let string_index = u32_at(data, type_offset)? as usize;
        let string_offset = *strings
            .get(string_index)
            .context("DEX 类型字符串索引越界")?;
        let descriptor = read_string(data, u32_at(data, string_offset)? as usize)?;
        let name = descriptor
            .strip_prefix('L')
            .and_then(|value| value.strip_suffix(';'))
            .filter(|value| !value.is_empty() && !value.contains('\0'))
            .context("DEX 类描述符无效")?;
        if !names.insert(name.to_string()) {
            bail!("DEX 存在重复类定义");
        }
    }
    Ok(names)
}

fn u32_at(data: &[u8], offset: usize) -> Result<u32> {
    let end = offset.checked_add(4).context("DEX 索引溢出")?;
    let bytes = data.get(offset..end).context("DEX 索引越界")?;
    Ok(u32::from_le_bytes(bytes.try_into()?))
}

fn table(data: &[u8], header: usize, width: usize) -> Result<Vec<usize>> {
    let count = u32_at(data, header)? as usize;
    let start = u32_at(data, header + 4)? as usize;
    if count != 0
        && (start < 112 || start & 3 != 0 || count > data.len().saturating_sub(start) / width)
    {
        bail!("DEX 索引表越界或未对齐");
    }
    Ok((0..count).map(|index| start + index * width).collect())
}

fn read_string(data: &[u8], mut cursor: usize) -> Result<String> {
    let mut count = 0u32;
    for index in 0..5 {
        let byte = *data.get(cursor).context("DEX 字符串长度截断")?;
        cursor += 1;
        if index == 4 && byte > 15 {
            bail!("DEX 字符串长度溢出");
        }
        count |= ((byte & 127) as u32) << (index * 7);
        if byte & 128 == 0 {
            let tail = data.get(cursor..).context("DEX 字符串偏移越界")?;
            let end = tail
                .iter()
                .position(|&byte| byte == 0)
                .context("DEX 字符串缺少结束标记")?;
            return decode_mutf8(&tail[..end], count as usize);
        }
    }
    bail!("DEX 字符串长度无效")
}

fn decode_mutf8(bytes: &[u8], count: usize) -> Result<String> {
    let mut units = Vec::new();
    let mut cursor = 0;
    while cursor < bytes.len() {
        let first = bytes[cursor];
        cursor += 1;
        let (mut value, extra, minimum) = match first {
            1..=127 => (first as u16, 0, 0),
            0xc0..=0xdf => ((first & 31) as u16, 1, 128),
            0xe0..=0xef => ((first & 15) as u16, 2, 2048),
            _ => bail!("DEX 类名 MUTF-8 编码无效"),
        };
        for _ in 0..extra {
            let byte = *bytes.get(cursor).context("DEX 类名 MUTF-8 截断")?;
            cursor += 1;
            if byte & 0xc0 != 0x80 {
                bail!("DEX 类名 MUTF-8 续字节无效");
            }
            value = (value << 6) | (byte & 63) as u16;
        }
        if value < minimum && !(value == 0 && extra == 1) {
            bail!("DEX 类名 MUTF-8 非规范编码");
        }
        units.push(value);
    }
    if units.len() != count {
        bail!("DEX 类名 UTF-16 长度不一致");
    }
    String::from_utf16(&units).context("DEX 类名 UTF-16 编码无效")
}

#[cfg(test)]
pub(crate) fn fixture(name: &str) -> Vec<u8> {
    let mut data = vec![0; 152];
    data[..8].copy_from_slice(b"dex\n035\0");
    for (offset, value) in [
        (36, 112),
        (40, 0x12345678),
        (56, 1),
        (60, 112),
        (64, 1),
        (68, 116),
        (96, 1),
        (100, 120),
        (112, 152),
    ] {
        data[offset..offset + 4].copy_from_slice(&u32::to_le_bytes(value));
    }
    let descriptor = format!("L{name};");
    assert!(descriptor.encode_utf16().count() < 128);
    data.push(descriptor.encode_utf16().count() as u8);
    data.extend_from_slice(descriptor.as_bytes());
    data.push(0);
    let size = data.len() as u32;
    data[32..36].copy_from_slice(&size.to_le_bytes());
    data
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 读取完整类名并支持中文() {
        assert!(defined_classes(&fixture("example/业务类"))
            .unwrap()
            .contains("example/业务类"));
    }

    #[test]
    fn 拒绝越界索引与表() {
        for offset in [60, 68, 100, 112, 116, 120] {
            let mut data = fixture("example/A");
            data[offset..offset + 4].copy_from_slice(&u32::MAX.to_le_bytes());
            assert!(defined_classes(&data).is_err(), "偏移 {offset}");
        }
    }

    #[test]
    fn 拒绝截断版本异常与字符串异常() {
        let data = fixture("example/A");
        assert!(defined_classes(&data[..data.len() - 1]).is_err());
        let mut bad_version = data.clone();
        bad_version[6] = b'1';
        assert!(defined_classes(&bad_version).is_err());
        let mut bad_length = data.clone();
        bad_length[152] += 1;
        assert!(defined_classes(&bad_length).is_err());
        let mut bad_end = data;
        *bad_end.last_mut().unwrap() = b'x';
        assert!(defined_classes(&bad_end).is_err());
    }

    #[test]
    fn 支持代理对并拒绝过长编码() {
        assert_eq!(
            decode_mutf8(&[0xed, 0xa0, 0xbd, 0xed, 0xb8, 0x80], 2).unwrap(),
            "😀"
        );
        assert!(decode_mutf8(&[0xc0, 0xaf], 1).is_err());
        assert!(decode_mutf8(&[0xed, 0xa0, 0x80], 1).is_err());
    }
}

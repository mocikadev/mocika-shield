use std::{fs::File, io::Read, path::Path};

#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct ManifestFacts {
    pub min_sdk: Option<u32>,
    pub target_sdk: Option<u32>,
    pub extract_native_libs: Option<bool>,
    pub split_name: Option<String>,
    pub uses_http_legacy: bool,
}

pub(crate) fn inspect_manifest(apk_path: &Path) -> Result<ManifestFacts, String> {
    let file = File::open(apk_path).map_err(|_| "无法读取 APK 文件".to_string())?;
    let mut archive = zip::ZipArchive::new(file).map_err(|_| "APK ZIP 结构无效".to_string())?;
    let mut manifest = archive
        .by_name("AndroidManifest.xml")
        .map_err(|_| "缺少 AndroidManifest.xml".to_string())?;
    let mut bytes = Vec::new();
    manifest
        .read_to_end(&mut bytes)
        .map_err(|_| "无法读取 AndroidManifest.xml".to_string())?;
    parse_manifest(&bytes)
}

fn parse_manifest(bytes: &[u8]) -> Result<ManifestFacts, String> {
    if bytes.starts_with(b"<") {
        return Ok(parse_text_manifest(
            std::str::from_utf8(bytes).map_err(|_| "Manifest 编码无效")?,
        ));
    }
    parse_binary_manifest(bytes)
}

fn parse_binary_manifest(bytes: &[u8]) -> Result<ManifestFacts, String> {
    let strings = parse_string_pool(bytes)?;
    let mut facts = ManifestFacts::default();
    let mut offset = 8usize;
    while offset + 8 <= bytes.len() {
        let chunk_type = read_u16(bytes, offset)?;
        let header_size = read_u16(bytes, offset + 2)? as usize;
        let chunk_size = read_u32(bytes, offset + 4)? as usize;
        if header_size < 8 || chunk_size < header_size || offset + chunk_size > bytes.len() {
            return Err("Manifest 块结构无效".to_string());
        }
        if chunk_type == 0x0102 {
            parse_start_element(bytes, offset, header_size, &strings, &mut facts)?;
        }
        offset += chunk_size;
    }
    Ok(facts)
}

fn parse_string_pool(bytes: &[u8]) -> Result<Vec<String>, String> {
    let mut offset = 8usize;
    while offset + 8 <= bytes.len() {
        let chunk_type = read_u16(bytes, offset)?;
        let header_size = read_u16(bytes, offset + 2)? as usize;
        let chunk_size = read_u32(bytes, offset + 4)? as usize;
        if header_size < 8 || chunk_size < header_size || offset + chunk_size > bytes.len() {
            return Err("Manifest 块结构无效".to_string());
        }
        if chunk_type == 0x0001 {
            return decode_string_pool(&bytes[offset..offset + chunk_size], header_size);
        }
        offset += chunk_size;
    }
    Err("Manifest 缺少字符串池".to_string())
}

fn decode_string_pool(chunk: &[u8], header_size: usize) -> Result<Vec<String>, String> {
    if header_size < 28 || chunk.len() < header_size {
        return Err("Manifest 字符串池无效".to_string());
    }
    let count = read_u32(chunk, 8)? as usize;
    let flags = read_u32(chunk, 16)?;
    let strings_start = read_u32(chunk, 20)? as usize;
    let offsets_end = header_size
        .checked_add(count.checked_mul(4).ok_or("Manifest 字符串池过大")?)
        .ok_or("Manifest 字符串池过大")?;
    if offsets_end > chunk.len() || strings_start > chunk.len() {
        return Err("Manifest 字符串池边界无效".to_string());
    }
    (0..count)
        .map(|index| {
            let relative = read_u32(chunk, header_size + index * 4)? as usize;
            let start = strings_start
                .checked_add(relative)
                .ok_or("Manifest 字符串偏移无效")?;
            if flags & 0x0000_0100 != 0 {
                decode_utf8_string(chunk, start)
            } else {
                decode_utf16_string(chunk, start)
            }
        })
        .collect()
}

fn parse_start_element(
    bytes: &[u8],
    offset: usize,
    header_size: usize,
    strings: &[String],
    facts: &mut ManifestFacts,
) -> Result<(), String> {
    if header_size < 16 {
        return Err("Manifest 元素结构无效".to_string());
    }
    let name = string_at(strings, read_u32(bytes, offset + 20)?)?;
    let attr_start = read_u16(bytes, offset + 24)? as usize;
    let attr_size = read_u16(bytes, offset + 26)? as usize;
    let attr_count = read_u16(bytes, offset + 28)? as usize;
    if attr_size < 20 {
        return Err("Manifest 属性结构无效".to_string());
    }
    let attrs_offset = offset
        .checked_add(16)
        .and_then(|value| value.checked_add(attr_start))
        .ok_or("Manifest 属性偏移无效")?;
    let attrs_end = attrs_offset
        .checked_add(
            attr_size
                .checked_mul(attr_count)
                .ok_or("Manifest 属性过多")?,
        )
        .ok_or("Manifest 属性过多")?;
    if attrs_end > bytes.len() {
        return Err("Manifest 属性边界无效".to_string());
    }
    for index in 0..attr_count {
        let attr = attrs_offset + index * attr_size;
        let attr_name = string_at(strings, read_u32(bytes, attr + 4)?)?;
        let raw_index = read_u32(bytes, attr + 8)?;
        let data_type = *bytes.get(attr + 15).ok_or("Manifest 属性值无效")?;
        let data = read_u32(bytes, attr + 16)?;
        let value = manifest_value(strings, raw_index, data_type, data)?;
        apply_attribute(name, attr_name, &value, facts);
    }
    Ok(())
}

fn manifest_value(
    strings: &[String],
    raw_index: u32,
    data_type: u8,
    data: u32,
) -> Result<String, String> {
    if raw_index != u32::MAX {
        return Ok(string_at(strings, raw_index)?.to_string());
    }
    match data_type {
        0x03 => Ok(string_at(strings, data)?.to_string()),
        0x10 => Ok(data.to_string()),
        0x12 => Ok((data != 0).to_string()),
        _ => Ok(String::new()),
    }
}

fn apply_attribute(element: &str, attribute: &str, value: &str, facts: &mut ManifestFacts) {
    match (element, attribute) {
        ("manifest", "split") if !value.is_empty() => facts.split_name = Some(value.to_string()),
        ("uses-sdk", "minSdkVersion") => facts.min_sdk = value.parse().ok(),
        ("uses-sdk", "targetSdkVersion") => facts.target_sdk = value.parse().ok(),
        ("application", "extractNativeLibs") => facts.extract_native_libs = parse_bool(value),
        ("uses-library", "name") if value == "org.apache.http.legacy" => {
            facts.uses_http_legacy = true
        }
        _ => {}
    }
}

fn parse_text_manifest(xml: &str) -> ManifestFacts {
    let mut facts = ManifestFacts::default();
    for tag in xml
        .split('<')
        .filter_map(|part| part.split_once('>').map(|(tag, _)| tag))
    {
        let tag = tag.trim();
        let element = tag
            .split_whitespace()
            .next()
            .unwrap_or_default()
            .trim_start_matches('/');
        if element == "manifest" {
            if !tag.starts_with('/') {
                facts.split_name = extract_attribute(tag, "split");
            }
        } else if element == "uses-sdk" {
            facts.min_sdk = extract_attribute(tag, "android:minSdkVersion")
                .and_then(|value| value.parse().ok());
            facts.target_sdk = extract_attribute(tag, "android:targetSdkVersion")
                .and_then(|value| value.parse().ok());
        } else if element == "application" {
            facts.extract_native_libs = extract_attribute(tag, "android:extractNativeLibs")
                .and_then(|value| parse_bool(&value));
        } else if element == "uses-library"
            && extract_attribute(tag, "android:name").as_deref() == Some("org.apache.http.legacy")
        {
            facts.uses_http_legacy = true;
        }
    }
    facts
}

fn extract_attribute(tag: &str, name: &str) -> Option<String> {
    let start = tag.find(name)? + name.len();
    let value = tag[start..].trim_start();
    let value = value.strip_prefix('=')?.trim_start();
    let quote = value.chars().next()?;
    if quote != '\'' && quote != '"' {
        return None;
    }
    value[1..]
        .find(quote)
        .map(|end| value[1..1 + end].to_string())
}

fn parse_bool(value: &str) -> Option<bool> {
    match value {
        "true" => Some(true),
        "false" => Some(false),
        _ => None,
    }
}

fn string_at(strings: &[String], index: u32) -> Result<&str, String> {
    strings
        .get(index as usize)
        .map(String::as_str)
        .ok_or_else(|| "Manifest 字符串索引无效".to_string())
}

fn read_u16(bytes: &[u8], offset: usize) -> Result<u16, String> {
    let value = bytes.get(offset..offset + 2).ok_or("Manifest 数据截断")?;
    Ok(u16::from_le_bytes([value[0], value[1]]))
}

fn read_u32(bytes: &[u8], offset: usize) -> Result<u32, String> {
    let value = bytes.get(offset..offset + 4).ok_or("Manifest 数据截断")?;
    Ok(u32::from_le_bytes([value[0], value[1], value[2], value[3]]))
}

fn decode_utf8_string(bytes: &[u8], offset: usize) -> Result<String, String> {
    let (_, after_utf16_len) = read_length8(bytes, offset)?;
    let (utf8_len, start) = read_length8(bytes, after_utf16_len)?;
    let end = start.checked_add(utf8_len).ok_or("Manifest 字符串过长")?;
    let value = bytes.get(start..end).ok_or("Manifest 字符串截断")?;
    String::from_utf8(value.to_vec()).map_err(|_| "Manifest UTF-8 字符串无效".to_string())
}

fn read_length8(bytes: &[u8], offset: usize) -> Result<(usize, usize), String> {
    let first = *bytes.get(offset).ok_or("Manifest 字符串截断")?;
    if first & 0x80 == 0 {
        Ok((first as usize, offset + 1))
    } else {
        let second = *bytes.get(offset + 1).ok_or("Manifest 字符串截断")?;
        Ok((((first as usize & 0x7f) << 8) | second as usize, offset + 2))
    }
}

fn decode_utf16_string(bytes: &[u8], offset: usize) -> Result<String, String> {
    let (len, start) = read_length16(bytes, offset)?;
    let byte_len = len.checked_mul(2).ok_or("Manifest 字符串过长")?;
    let end = start.checked_add(byte_len).ok_or("Manifest 字符串过长")?;
    let value = bytes.get(start..end).ok_or("Manifest 字符串截断")?;
    let units = value
        .chunks_exact(2)
        .map(|part| u16::from_le_bytes([part[0], part[1]]))
        .collect::<Vec<_>>();
    String::from_utf16(&units).map_err(|_| "Manifest UTF-16 字符串无效".to_string())
}

fn read_length16(bytes: &[u8], offset: usize) -> Result<(usize, usize), String> {
    let first = read_u16(bytes, offset)?;
    if first & 0x8000 == 0 {
        Ok((first as usize, offset + 2))
    } else {
        let second = read_u16(bytes, offset + 2)?;
        Ok((
            ((first as usize & 0x7fff) << 16) | second as usize,
            offset + 4,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 解析文本_manifest_的安装与兼容字段() {
        assert_eq!(
            extract_attribute(r#"manifest split="config.arm64_v8a""#, "split").as_deref(),
            Some("config.arm64_v8a")
        );
        let facts = parse_text_manifest(
            r#"<manifest split="config.arm64_v8a"><uses-sdk android:minSdkVersion="21" android:targetSdkVersion="35"/><application android:extractNativeLibs="false"/><uses-library android:name="org.apache.http.legacy"/></manifest>"#,
        );
        assert_eq!(facts.min_sdk, Some(21));
        assert_eq!(facts.target_sdk, Some(35));
        assert_eq!(facts.extract_native_libs, Some(false));
        assert_eq!(facts.split_name.as_deref(), Some("config.arm64_v8a"));
        assert!(facts.uses_http_legacy);
    }

    #[test]
    fn 解析二进制_manifest_的安装与兼容字段() {
        let strings = [
            "manifest",
            "split",
            "config.arm64_v8a",
            "uses-sdk",
            "minSdkVersion",
            "21",
            "targetSdkVersion",
            "35",
            "application",
            "extractNativeLibs",
            "false",
            "uses-library",
            "name",
            "org.apache.http.legacy",
        ];
        let mut chunks = string_pool(&strings);
        chunks.extend(start_element(0, &[(1, 2)]));
        chunks.extend(start_element(3, &[(4, 5), (6, 7)]));
        chunks.extend(start_element(8, &[(9, 10)]));
        chunks.extend(start_element(11, &[(12, 13)]));
        let mut manifest = Vec::new();
        push_u16(&mut manifest, 0x0003);
        push_u16(&mut manifest, 8);
        push_u32(&mut manifest, (8 + chunks.len()) as u32);
        manifest.extend(chunks);

        let facts = parse_binary_manifest(&manifest).unwrap();
        assert_eq!(facts.min_sdk, Some(21));
        assert_eq!(facts.target_sdk, Some(35));
        assert_eq!(facts.extract_native_libs, Some(false));
        assert_eq!(facts.split_name.as_deref(), Some("config.arm64_v8a"));
        assert!(facts.uses_http_legacy);
    }

    fn string_pool(strings: &[&str]) -> Vec<u8> {
        let mut data = Vec::new();
        let mut offsets = Vec::new();
        for value in strings {
            offsets.push(data.len() as u32);
            data.push(value.len() as u8);
            data.push(value.len() as u8);
            data.extend(value.as_bytes());
            data.push(0);
        }
        let header_size = 28usize;
        let strings_start = header_size + offsets.len() * 4;
        let mut chunk = Vec::new();
        push_u16(&mut chunk, 0x0001);
        push_u16(&mut chunk, header_size as u16);
        push_u32(&mut chunk, (strings_start + data.len()) as u32);
        push_u32(&mut chunk, strings.len() as u32);
        push_u32(&mut chunk, 0);
        push_u32(&mut chunk, 0x0000_0100);
        push_u32(&mut chunk, strings_start as u32);
        push_u32(&mut chunk, 0);
        for offset in offsets {
            push_u32(&mut chunk, offset);
        }
        chunk.extend(data);
        chunk
    }

    fn start_element(name: u32, attributes: &[(u32, u32)]) -> Vec<u8> {
        let mut chunk = Vec::new();
        push_u16(&mut chunk, 0x0102);
        push_u16(&mut chunk, 16);
        push_u32(&mut chunk, (36 + attributes.len() * 20) as u32);
        push_u32(&mut chunk, 0);
        push_u32(&mut chunk, u32::MAX);
        push_u32(&mut chunk, u32::MAX);
        push_u32(&mut chunk, name);
        push_u16(&mut chunk, 20);
        push_u16(&mut chunk, 20);
        push_u16(&mut chunk, attributes.len() as u16);
        push_u16(&mut chunk, 0);
        push_u16(&mut chunk, 0);
        push_u16(&mut chunk, 0);
        for (attribute_name, value) in attributes {
            push_u32(&mut chunk, u32::MAX);
            push_u32(&mut chunk, *attribute_name);
            push_u32(&mut chunk, *value);
            push_u16(&mut chunk, 8);
            chunk.push(0);
            chunk.push(0x03);
            push_u32(&mut chunk, *value);
        }
        chunk
    }

    fn push_u16(bytes: &mut Vec<u8>, value: u16) {
        bytes.extend(value.to_le_bytes());
    }

    fn push_u32(bytes: &mut Vec<u8>, value: u32) {
        bytes.extend(value.to_le_bytes());
    }
}

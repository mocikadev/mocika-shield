use std::{fs::File, io::Read, path::Path};

const ANDROID_NAMESPACE: &str = "http://schemas.android.com/apk/res/android";
const STRICT_STRING_COUNT_LIMIT: usize = 65_536;
const STRICT_STRING_BYTES_LIMIT: usize = 4 * 1024 * 1024;

#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct ManifestFacts {
    pub min_sdk: Option<u32>,
    pub target_sdk: Option<u32>,
    pub extract_native_libs: Option<bool>,
    pub split_name: Option<String>,
    pub uses_http_legacy: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ManifestLabel {
    Text(String),
    Resource(u32),
    Missing,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ApplicationManifestMetadata {
    pub package_name: String,
    pub label: ManifestLabel,
    pub version_code: Option<u32>,
    pub version_code_major: Option<u32>,
}

impl Default for ApplicationManifestMetadata {
    fn default() -> Self {
        Self {
            package_name: String::new(),
            label: ManifestLabel::Missing,
            version_code: None,
            version_code_major: None,
        }
    }
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
    parse_binary_manifest_all(bytes).map(|(facts, _)| facts)
}

pub(crate) fn parse_application_metadata(
    bytes: &[u8],
) -> Result<ApplicationManifestMetadata, String> {
    if bytes.starts_with(b"<") {
        return Err("文本 Manifest 不能作为可靠应用身份来源".to_string());
    }
    validate_strict_binary_xml_header(bytes)?;
    parse_binary_application_metadata(bytes)
}

fn validate_strict_binary_xml_header(bytes: &[u8]) -> Result<(), String> {
    if bytes.len() < 8
        || read_u16(bytes, 0)? != 0x0003
        || read_u16(bytes, 2)? != 8
        || read_u32(bytes, 4)? as usize != bytes.len()
    {
        return Err("Manifest 二进制 XML 外层结构无效".to_string());
    }
    Ok(())
}

fn parse_binary_application_metadata(bytes: &[u8]) -> Result<ApplicationManifestMetadata, String> {
    let strings = parse_strict_string_pool(bytes)?;
    let mut metadata = ApplicationManifestMetadata::default();
    let mut stack = Vec::<(u32, u32)>::new();
    let mut root_seen = false;
    let mut root_closed = false;
    let mut application_seen = false;
    let mut offset = 8usize;
    while offset + 8 <= bytes.len() {
        let chunk_type = read_u16(bytes, offset)?;
        let header_size = read_u16(bytes, offset + 2)? as usize;
        let chunk_size = read_u32(bytes, offset + 4)? as usize;
        let chunk_end = offset
            .checked_add(chunk_size)
            .ok_or("Manifest 块边界溢出")?;
        if header_size < 8 || chunk_size < header_size || chunk_end > bytes.len() {
            return Err("Manifest 块结构无效".to_string());
        }
        match chunk_type {
            0x0102 => {
                let fixed_end = offset.checked_add(36).ok_or("Manifest 元素边界溢出")?;
                if chunk_size < 36 || fixed_end > chunk_end {
                    return Err("Manifest 开始元素结构无效".to_string());
                }
                let namespace = read_u32(bytes, offset + 16)?;
                let name_index = read_u32(bytes, offset + 20)?;
                let name = string_at(&strings, name_index)?;
                let namespace_name = optional_string_at(&strings, namespace)?;
                let depth = stack.len();
                if name == "manifest" {
                    if depth != 0 || root_seen || root_closed || namespace_name.is_some() {
                        return Err("Manifest 根节点不唯一或命名空间无效".to_string());
                    }
                    root_seen = true;
                    parse_identity_attributes(
                        bytes,
                        offset,
                        header_size,
                        chunk_end,
                        &strings,
                        IdentityElement::Manifest,
                        &mut metadata,
                    )?;
                } else if depth == 1 && name == "application" && namespace_name.is_none() {
                    if application_seen {
                        return Err("Manifest 包含多个直属 application".to_string());
                    }
                    application_seen = true;
                    parse_identity_attributes(
                        bytes,
                        offset,
                        header_size,
                        chunk_end,
                        &strings,
                        IdentityElement::Application,
                        &mut metadata,
                    )?;
                } else if depth == 0 {
                    return Err("Manifest 根节点无效".to_string());
                }
                stack.push((namespace, name_index));
            }
            0x0103 => {
                let fixed_end = offset.checked_add(24).ok_or("Manifest 元素边界溢出")?;
                if chunk_size < 24 || fixed_end > chunk_end {
                    return Err("Manifest 结束元素结构无效".to_string());
                }
                let namespace = read_u32(bytes, offset + 16)?;
                let name = read_u32(bytes, offset + 20)?;
                if stack.pop() != Some((namespace, name)) {
                    return Err("Manifest 元素闭合结构无效".to_string());
                }
                if stack.is_empty() {
                    root_closed = true;
                }
            }
            _ => {}
        }
        offset = chunk_end;
    }
    if offset != bytes.len()
        || !root_seen
        || !root_closed
        || !stack.is_empty()
        || metadata.package_name.is_empty()
    {
        return Err("Manifest 身份结构不完整".to_string());
    }
    Ok(metadata)
}

#[derive(Clone, Copy)]
enum IdentityElement {
    Manifest,
    Application,
}

fn parse_identity_attributes(
    bytes: &[u8],
    offset: usize,
    header_size: usize,
    chunk_end: usize,
    strings: &[String],
    element: IdentityElement,
    metadata: &mut ApplicationManifestMetadata,
) -> Result<(), String> {
    let attr_start = read_u16(bytes, offset + 24)? as usize;
    let attr_size = read_u16(bytes, offset + 26)? as usize;
    let attr_count = read_u16(bytes, offset + 28)? as usize;
    if header_size < 16 || attr_size < 20 {
        return Err("Manifest 属性结构无效".to_string());
    }
    let attrs_offset = offset
        .checked_add(16)
        .and_then(|v| v.checked_add(attr_start))
        .ok_or("Manifest 属性偏移无效")?;
    let attrs_end = attrs_offset
        .checked_add(
            attr_size
                .checked_mul(attr_count)
                .ok_or("Manifest 属性过多")?,
        )
        .ok_or("Manifest 属性过多")?;
    let fixed_end = offset.checked_add(36).ok_or("Manifest 元素边界溢出")?;
    if attrs_offset < fixed_end || attrs_end > chunk_end {
        return Err("Manifest 属性边界无效".to_string());
    }
    for index in 0..attr_count {
        let attr = attrs_offset + index * attr_size;
        let namespace = optional_string_at(strings, read_u32(bytes, attr)?)?;
        let name = string_at(strings, read_u32(bytes, attr + 4)?)?;
        let raw = read_u32(bytes, attr + 8)?;
        let data_type = *bytes.get(attr + 15).ok_or("Manifest 属性值无效")?;
        let data = read_u32(bytes, attr + 16)?;
        let value = manifest_value(strings, raw, data_type, data)?;
        match (element, namespace, name) {
            (IdentityElement::Manifest, None, "package") => {
                if !metadata.package_name.is_empty() {
                    return Err("Manifest 包名属性重复".to_string());
                }
                metadata.package_name = value;
            }
            (IdentityElement::Manifest, Some(ANDROID_NAMESPACE), "versionCode") => {
                if metadata.version_code.is_some() {
                    return Err("Manifest 版本码属性重复".to_string());
                }
                metadata.version_code = typed_u32(raw, data_type, data, &value)
            }
            (IdentityElement::Manifest, Some(ANDROID_NAMESPACE), "versionCodeMajor") => {
                if metadata.version_code_major.is_some() {
                    return Err("Manifest 主版本码属性重复".to_string());
                }
                metadata.version_code_major = typed_u32(raw, data_type, data, &value)
            }
            (IdentityElement::Application, Some(ANDROID_NAMESPACE), "label")
                if raw != u32::MAX || data_type == 0x03 =>
            {
                if metadata.label != ManifestLabel::Missing {
                    return Err("Manifest 应用名称属性重复".to_string());
                }
                metadata.label = ManifestLabel::Text(value)
            }
            (IdentityElement::Application, Some(ANDROID_NAMESPACE), "label")
                if data_type == 0x01 =>
            {
                if metadata.label != ManifestLabel::Missing {
                    return Err("Manifest 应用名称属性重复".to_string());
                }
                metadata.label = ManifestLabel::Resource(data)
            }
            _ => {}
        }
    }
    Ok(())
}

fn parse_strict_string_pool(bytes: &[u8]) -> Result<Vec<String>, String> {
    let mut offset = 8usize;
    while offset + 8 <= bytes.len() {
        let chunk_type = read_u16(bytes, offset)?;
        let header_size = read_u16(bytes, offset + 2)? as usize;
        let chunk_size = read_u32(bytes, offset + 4)? as usize;
        let chunk_end = offset
            .checked_add(chunk_size)
            .ok_or("Manifest 块边界溢出")?;
        if header_size < 8 || chunk_size < header_size || chunk_end > bytes.len() {
            return Err("Manifest 块结构无效".to_string());
        }
        if chunk_type == 0x0001 {
            return decode_strict_string_pool(&bytes[offset..chunk_end], header_size);
        }
        offset = chunk_end;
    }
    Err("Manifest 缺少字符串池".to_string())
}

fn decode_strict_string_pool(chunk: &[u8], header_size: usize) -> Result<Vec<String>, String> {
    if header_size < 28 || chunk.len() < header_size {
        return Err("Manifest 字符串池无效".to_string());
    }
    let count = read_u32(chunk, 8)? as usize;
    if count > STRICT_STRING_COUNT_LIMIT {
        return Err("Manifest 字符串数量超过上限".to_string());
    }
    let flags = read_u32(chunk, 16)?;
    let strings_start = read_u32(chunk, 20)? as usize;
    let offsets_end = header_size
        .checked_add(count.checked_mul(4).ok_or("Manifest 字符串池过大")?)
        .ok_or("Manifest 字符串池过大")?;
    if offsets_end > chunk.len() || strings_start > chunk.len() {
        return Err("Manifest 字符串池边界无效".to_string());
    }
    let mut total_bytes = 0usize;
    (0..count)
        .map(|index| {
            let relative = read_u32(chunk, header_size + index * 4)? as usize;
            let start = strings_start
                .checked_add(relative)
                .ok_or("Manifest 字符串偏移无效")?;
            let value = if flags & 0x0000_0100 != 0 {
                decode_utf8_string(chunk, start)
            } else {
                decode_utf16_string(chunk, start)
            }?;
            total_bytes = total_bytes
                .checked_add(value.len())
                .ok_or("Manifest 字符串累计大小溢出")?;
            if total_bytes > STRICT_STRING_BYTES_LIMIT {
                return Err("Manifest 字符串累计大小超过上限".to_string());
            }
            Ok(value)
        })
        .collect()
}

fn optional_string_at(strings: &[String], index: u32) -> Result<Option<&str>, String> {
    if index == u32::MAX {
        Ok(None)
    } else {
        string_at(strings, index).map(Some)
    }
}

fn parse_binary_manifest_all(
    bytes: &[u8],
) -> Result<(ManifestFacts, ApplicationManifestMetadata), String> {
    let strings = parse_string_pool(bytes)?;
    let mut facts = ManifestFacts::default();
    let mut metadata = ApplicationManifestMetadata::default();
    let mut offset = 8usize;
    while offset + 8 <= bytes.len() {
        let chunk_type = read_u16(bytes, offset)?;
        let header_size = read_u16(bytes, offset + 2)? as usize;
        let chunk_size = read_u32(bytes, offset + 4)? as usize;
        if header_size < 8 || chunk_size < header_size || offset + chunk_size > bytes.len() {
            return Err("Manifest 块结构无效".to_string());
        }
        if chunk_type == 0x0102 {
            parse_start_element(
                bytes,
                offset,
                header_size,
                &strings,
                &mut facts,
                &mut metadata,
            )?;
        }
        offset += chunk_size;
    }
    Ok((facts, metadata))
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
    metadata: &mut ApplicationManifestMetadata,
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
        apply_application_attribute(
            name, attr_name, raw_index, data_type, data, &value, metadata,
        );
    }
    Ok(())
}

fn apply_application_attribute(
    element: &str,
    attribute: &str,
    raw_index: u32,
    data_type: u8,
    data: u32,
    value: &str,
    metadata: &mut ApplicationManifestMetadata,
) {
    match (element, attribute) {
        ("manifest", "package") => metadata.package_name = value.to_string(),
        ("manifest", "versionCode") => {
            metadata.version_code = typed_u32(raw_index, data_type, data, value)
        }
        ("manifest", "versionCodeMajor") => {
            metadata.version_code_major = typed_u32(raw_index, data_type, data, value)
        }
        ("application", "label") if raw_index != u32::MAX || data_type == 0x03 => {
            metadata.label = ManifestLabel::Text(value.to_string())
        }
        ("application", "label") if data_type == 0x01 => {
            metadata.label = ManifestLabel::Resource(data)
        }
        _ => {}
    }
}

fn typed_u32(raw_index: u32, data_type: u8, data: u32, value: &str) -> Option<u32> {
    if raw_index == u32::MAX && matches!(data_type, 0x10 | 0x11) {
        Some(data)
    } else {
        value.parse().ok()
    }
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
    use std::io::Write;

    #[test]
    fn application_文本清单不作为可靠分享身份来源() {
        assert!(parse_application_metadata(
            br#"<manifest package="com.example.fake"><application android:label="fake"/></manifest>"#
        )
        .is_err());
    }

    #[test]
    fn application_严格清单字符串池重复偏移受累计预算限制() {
        let value = "x".repeat(100);
        let pool = repeated_offset_string_pool(45_000, &value);
        let mut xml = vec![0u8; 8];
        xml.extend(pool);
        assert!(parse_strict_string_pool(&xml).is_err());
    }

    #[test]
    fn application_严格属性区域不能越过当前元素块() {
        let strings = vec![
            "manifest".to_string(),
            "package".to_string(),
            "com.example.app".to_string(),
        ];
        let mut element = start_element(0, &[(1, 2)]);
        let declared_end = 16usize;
        assert!(parse_identity_attributes(
            &element,
            0,
            16,
            declared_end,
            &strings,
            IdentityElement::Manifest,
            &mut ApplicationManifestMetadata::default(),
        )
        .is_err());
        element.truncate(declared_end);
    }

    #[test]
    fn application_严格开始结束元素固定字段必须位于当前块内() {
        let valid = application_manifest_fixture("com.example.demo", "名称", 1, 0);

        let mut short_start = valid.clone();
        let start = find_chunk_offset(&short_start, 0x0102).expect("应包含开始元素");
        short_start[start + 4..start + 8].copy_from_slice(&24u32.to_le_bytes());
        assert!(parse_application_metadata(&short_start)
            .expect_err("短开始元素应失败")
            .contains("开始元素"));

        let mut short_end = valid;
        let end = find_chunk_offset(&short_end, 0x0103).expect("应包含结束元素");
        short_end[end + 4..end + 8].copy_from_slice(&16u32.to_le_bytes());
        assert!(parse_application_metadata(&short_end)
            .expect_err("短结束元素应失败")
            .contains("结束元素"));
    }

    #[test]
    fn application_重复根节点与嵌套应用不能覆盖可靠身份() {
        assert!(parse_application_metadata(&invalid_repeated_manifest_fixture()).is_err());
        let nested = parse_application_metadata(&nested_application_fixture())
            .expect("唯一根节点仍应可解析");
        assert_eq!(nested.label, ManifestLabel::Missing);
    }

    #[test]
    fn application_非_android_命名空间属性不能成为版本或名称() {
        let metadata =
            parse_application_metadata(&application_manifest_without_android_namespace_fixture())
                .expect("根包名应可靠");
        assert_eq!(metadata.version_code, None);
        assert_eq!(metadata.version_code_major, None);
        assert_eq!(metadata.label, ManifestLabel::Missing);
    }

    #[test]
    fn application_包名必须无命名空间且_application_元素必须无命名空间() {
        assert!(parse_application_metadata(&namespaced_package_fixture()).is_err());
        let metadata = parse_application_metadata(&namespaced_application_fixture())
            .expect("可靠根包名仍应解析");
        assert_eq!(metadata.label, ManifestLabel::Missing);
    }

    #[test]
    fn application_原清单预检保持超过八兆的既有读取行为() {
        let dir = tempfile::tempdir().expect("应创建临时目录");
        let path = dir.path().join("large.apk");
        let file = File::create(&path).expect("应创建 APK");
        let mut archive = zip::ZipWriter::new(file);
        archive
            .start_file(
                "AndroidManifest.xml",
                zip::write::SimpleFileOptions::default(),
            )
            .expect("应写入清单");
        let mut xml = br#"<manifest><uses-sdk android:minSdkVersion="21"/>"#.to_vec();
        xml.resize(8 * 1024 * 1024 + 1, b' ');
        xml.extend(b"</manifest>");
        archive.write_all(&xml).expect("应写入超大清单");
        archive.finish().expect("应完成 APK");

        let facts = inspect_manifest(&path).expect("原预检入口不应受分享读取上限影响");
        assert_eq!(facts.min_sdk, Some(21));
    }

    #[test]
    fn application_二进制清单读取包名直接名称和合并版本码() {
        let manifest = application_manifest_fixture("com.example.demo", "演示应用", u32::MAX, 1);
        let metadata = parse_application_metadata(&manifest).expect("应用元数据应可解析");
        assert_eq!(metadata.package_name, "com.example.demo");
        assert_eq!(metadata.label, ManifestLabel::Text("演示应用".to_string()));
        assert_eq!(metadata.version_code, Some(u32::MAX));
        assert_eq!(metadata.version_code_major, Some(1));
    }

    #[test]
    fn application_严格身份拒绝损坏的二进制_xml_外层头和尾部() {
        let valid = application_manifest_fixture("com.example.demo", "演示应用", 1, 0);
        let mut invalid_type = valid.clone();
        invalid_type[..2].copy_from_slice(&0u16.to_le_bytes());
        assert!(parse_application_metadata(&invalid_type).is_err());

        let mut invalid_header_size = valid.clone();
        invalid_header_size[2..4].copy_from_slice(&0u16.to_le_bytes());
        assert!(parse_application_metadata(&invalid_header_size).is_err());

        let mut invalid_total_size = valid.clone();
        invalid_total_size[4..8].copy_from_slice(&8u32.to_le_bytes());
        assert!(parse_application_metadata(&invalid_total_size).is_err());

        let mut trailing_byte = valid;
        trailing_byte.push(0xff);
        let trailing_size = trailing_byte.len() as u32;
        trailing_byte[4..8].copy_from_slice(&trailing_size.to_le_bytes());
        assert!(parse_application_metadata(&trailing_byte).is_err());
    }

    fn application_manifest_fixture(
        package_name: &str,
        label: &str,
        version_code: u32,
        version_code_major: u32,
    ) -> Vec<u8> {
        let strings = [
            "manifest",
            "package",
            package_name,
            "versionCode",
            "versionCodeMajor",
            "application",
            "label",
            label,
            ANDROID_NAMESPACE,
        ];
        let mut chunks = string_pool(&strings);
        chunks.extend(start_element_typed(
            0,
            u32::MAX,
            &[
                (u32::MAX, 1, 2, 0x03, 2),
                (8, 3, u32::MAX, 0x10, version_code),
                (8, 4, u32::MAX, 0x10, version_code_major),
            ],
        ));
        chunks.extend(start_element_typed(5, u32::MAX, &[(8, 6, 7, 0x03, 7)]));
        chunks.extend(end_element(u32::MAX, 5));
        chunks.extend(end_element(u32::MAX, 0));
        let mut manifest = Vec::new();
        push_u16(&mut manifest, 0x0003);
        push_u16(&mut manifest, 8);
        push_u32(&mut manifest, (8 + chunks.len()) as u32);
        manifest.extend(chunks);
        manifest
    }

    fn application_manifest_without_android_namespace_fixture() -> Vec<u8> {
        let strings = [
            "manifest",
            "package",
            "com.example.demo",
            "versionCode",
            "application",
            "label",
            "名称",
        ];
        let mut chunks = string_pool(&strings);
        chunks.extend(start_element_typed(
            0,
            u32::MAX,
            &[(u32::MAX, 1, 2, 0x03, 2), (u32::MAX, 3, u32::MAX, 0x10, 7)],
        ));
        chunks.extend(start_element_typed(
            4,
            u32::MAX,
            &[(u32::MAX, 5, 6, 0x03, 6)],
        ));
        chunks.extend(end_element(u32::MAX, 4));
        chunks.extend(end_element(u32::MAX, 0));
        binary_xml(chunks)
    }

    fn namespaced_package_fixture() -> Vec<u8> {
        let strings = ["manifest", "package", "com.example.demo", ANDROID_NAMESPACE];
        let mut chunks = string_pool(&strings);
        chunks.extend(start_element_typed(0, u32::MAX, &[(3, 1, 2, 0x03, 2)]));
        chunks.extend(end_element(u32::MAX, 0));
        binary_xml(chunks)
    }

    fn namespaced_application_fixture() -> Vec<u8> {
        let strings = [
            "manifest",
            "package",
            "com.example.demo",
            "application",
            "label",
            "名称",
            ANDROID_NAMESPACE,
        ];
        let mut chunks = string_pool(&strings);
        chunks.extend(start_element_typed(
            0,
            u32::MAX,
            &[(u32::MAX, 1, 2, 0x03, 2)],
        ));
        chunks.extend(start_element_typed(3, 6, &[(6, 4, 5, 0x03, 5)]));
        chunks.extend(end_element(6, 3));
        chunks.extend(end_element(u32::MAX, 0));
        binary_xml(chunks)
    }

    fn invalid_repeated_manifest_fixture() -> Vec<u8> {
        let strings = [
            "manifest",
            "package",
            "com.example.root",
            "com.example.fake",
        ];
        let mut chunks = string_pool(&strings);
        chunks.extend(start_element(0, &[(1, 2)]));
        chunks.extend(end_element(u32::MAX, 0));
        chunks.extend(start_element(0, &[(1, 3)]));
        chunks.extend(end_element(u32::MAX, 0));
        binary_xml(chunks)
    }

    fn nested_application_fixture() -> Vec<u8> {
        let strings = [
            "manifest",
            "package",
            "com.example.root",
            "node",
            "application",
            "label",
            "伪造名称",
        ];
        let mut chunks = string_pool(&strings);
        chunks.extend(start_element(0, &[(1, 2)]));
        chunks.extend(start_element(3, &[]));
        chunks.extend(start_element(4, &[(5, 6)]));
        chunks.extend(end_element(u32::MAX, 4));
        chunks.extend(end_element(u32::MAX, 3));
        chunks.extend(end_element(u32::MAX, 0));
        binary_xml(chunks)
    }

    fn binary_xml(chunks: Vec<u8>) -> Vec<u8> {
        let mut manifest = Vec::new();
        push_u16(&mut manifest, 0x0003);
        push_u16(&mut manifest, 8);
        push_u32(&mut manifest, (8 + chunks.len()) as u32);
        manifest.extend(chunks);
        manifest
    }

    fn start_element_typed(
        name: u32,
        namespace: u32,
        attributes: &[(u32, u32, u32, u8, u32)],
    ) -> Vec<u8> {
        let mut chunk = Vec::new();
        push_u16(&mut chunk, 0x0102);
        push_u16(&mut chunk, 16);
        push_u32(&mut chunk, (36 + attributes.len() * 20) as u32);
        push_u32(&mut chunk, 0);
        push_u32(&mut chunk, u32::MAX);
        push_u32(&mut chunk, namespace);
        push_u32(&mut chunk, name);
        push_u16(&mut chunk, 20);
        push_u16(&mut chunk, 20);
        push_u16(&mut chunk, attributes.len() as u16);
        push_u16(&mut chunk, 0);
        push_u16(&mut chunk, 0);
        push_u16(&mut chunk, 0);
        for (attribute_namespace, attribute_name, raw, data_type, data) in attributes {
            push_u32(&mut chunk, *attribute_namespace);
            push_u32(&mut chunk, *attribute_name);
            push_u32(&mut chunk, *raw);
            push_u16(&mut chunk, 8);
            chunk.push(0);
            chunk.push(*data_type);
            push_u32(&mut chunk, *data);
        }
        chunk
    }

    fn end_element(namespace: u32, name: u32) -> Vec<u8> {
        let mut chunk = Vec::new();
        push_u16(&mut chunk, 0x0103);
        push_u16(&mut chunk, 16);
        push_u32(&mut chunk, 24);
        push_u32(&mut chunk, 0);
        push_u32(&mut chunk, u32::MAX);
        push_u32(&mut chunk, namespace);
        push_u32(&mut chunk, name);
        chunk
    }

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

    fn repeated_offset_string_pool(count: usize, value: &str) -> Vec<u8> {
        let strings_start = 28 + count * 4;
        let mut chunk = Vec::new();
        push_u16(&mut chunk, 0x0001);
        push_u16(&mut chunk, 28);
        push_u32(&mut chunk, (strings_start + value.len() + 3) as u32);
        push_u32(&mut chunk, count as u32);
        push_u32(&mut chunk, 0);
        push_u32(&mut chunk, 0x0000_0100);
        push_u32(&mut chunk, strings_start as u32);
        push_u32(&mut chunk, 0);
        for _ in 0..count {
            push_u32(&mut chunk, 0);
        }
        chunk.push(value.chars().count() as u8);
        chunk.push(value.len() as u8);
        chunk.extend(value.as_bytes());
        chunk.push(0);
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

    fn find_chunk_offset(bytes: &[u8], kind: u16) -> Option<usize> {
        bytes
            .windows(2)
            .position(|window| window == kind.to_le_bytes())
    }
}

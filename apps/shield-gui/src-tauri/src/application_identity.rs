use crate::manifest_inspect::{parse_application_metadata, ManifestLabel};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::fs::File;
use std::io::Read;
use std::path::Path;

const MANIFEST_LIMIT: u64 = 8 * 1024 * 1024;
const RESOURCES_LIMIT: u64 = 32 * 1024 * 1024;
const RESOURCE_STRING_COUNT_LIMIT: usize = 65_536;
const RESOURCE_STRING_BYTES_LIMIT: usize = 4 * 1024 * 1024;
const RESOURCE_ENTRY_COUNT_LIMIT: usize = 65_536;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct ApplicationIdentity {
    pub package_name: String,
    pub app_name: Option<String>,
    pub app_version_code: Option<String>,
}

pub(crate) type LabelValue = ManifestLabel;

pub(crate) fn read_application_identity(apk_path: &Path) -> Result<ApplicationIdentity, String> {
    let file = File::open(apk_path).map_err(|_| "无法读取 APK 文件".to_string())?;
    let mut archive = zip::ZipArchive::new(file).map_err(|_| "APK ZIP 结构无效".to_string())?;
    let manifest = read_bounded_entry(&mut archive, "AndroidManifest.xml", MANIFEST_LIMIT)?;
    let metadata = parse_application_metadata(&manifest)?;
    let resources = match &metadata.label {
        ManifestLabel::Resource(_) => archive
            .by_name("resources.arsc")
            .map_err(|_| "应用名称引用资源，但 APK 缺少 resources.arsc".to_string())
            .and_then(|mut entry| {
                validate_entry_size("resources.arsc", entry.size())?;
                read_limited(&mut entry, "resources.arsc", RESOURCES_LIMIT)
            })
            .ok(),
        _ => None,
    };
    identity_from_parts(
        &metadata.package_name,
        Some(metadata.label),
        metadata.version_code,
        metadata.version_code_major,
        resources.as_deref(),
    )
}

fn read_bounded_entry(
    archive: &mut zip::ZipArchive<File>,
    name: &str,
    limit: u64,
) -> Result<Vec<u8>, String> {
    let mut entry = archive.by_name(name).map_err(|_| format!("缺少 {name}"))?;
    if entry.size() > limit {
        return Err(format!("{name} 超过读取上限"));
    }
    read_limited(&mut entry, name, limit)
}

fn read_limited(reader: impl Read, name: &str, limit: u64) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    reader
        .take(limit.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|_| format!("无法读取 {name}"))?;
    if bytes.len() as u64 > limit {
        Err(format!("{name} 超过读取上限"))
    } else {
        Ok(bytes)
    }
}

fn validate_entry_size(name: &str, size: u64) -> Result<(), String> {
    let limit = match name {
        "AndroidManifest.xml" => MANIFEST_LIMIT,
        "resources.arsc" => RESOURCES_LIMIT,
        _ => return Err("不支持的 APK 条目".to_string()),
    };
    if size > limit {
        Err(format!("{name} 超过读取上限"))
    } else {
        Ok(())
    }
}

fn identity_from_parts(
    package_name: &str,
    label: Option<LabelValue>,
    version_code: Option<u32>,
    version_code_major: Option<u32>,
    resources: Option<&[u8]>,
) -> Result<ApplicationIdentity, String> {
    validate_package_name(package_name)?;
    let app_name = match label.unwrap_or(ManifestLabel::Missing) {
        ManifestLabel::Text(value) => valid_app_name(value),
        ManifestLabel::Resource(id) => {
            resources.and_then(|bytes| resolve_resource_label(bytes, id))
        }
        ManifestLabel::Missing => None,
    };
    let combined = (version_code_major.unwrap_or(0) as u64)
        .checked_shl(32)
        .and_then(|major| major.checked_add(version_code.unwrap_or(0) as u64));
    let app_version_code = if version_code.is_none() && version_code_major.is_none() {
        None
    } else {
        combined
            .filter(|value| *value <= i64::MAX as u64)
            .map(|value| value.to_string())
    };
    Ok(ApplicationIdentity {
        package_name: package_name.to_string(),
        app_name,
        app_version_code,
    })
}

fn validate_package_name(value: &str) -> Result<(), String> {
    if value.is_empty() || value.len() > 255 || !value.is_ascii() {
        return Err("应用包名无效".to_string());
    }
    let valid = value.split('.').all(|segment| {
        let mut chars = segment.chars();
        matches!(chars.next(), Some(first) if first.is_ascii_alphabetic() || first == '_')
            && chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
    });
    if valid {
        Ok(())
    } else {
        Err("应用包名无效".to_string())
    }
}

fn valid_app_name(value: String) -> Option<String> {
    (value.chars().count() <= 256 && value.len() <= 1024 && !value.chars().any(char::is_control))
        .then_some(value)
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum ResourceValue {
    StringIndex(u32),
    Reference(u32),
}

struct ResourceParseState<'a> {
    strings: &'a [String],
    output: &'a mut BTreeMap<u32, BTreeMap<u8, Vec<ResourceValue>>>,
    total_entries: &'a mut usize,
}

struct ResourceTable {
    strings: Vec<String>,
    values: BTreeMap<u32, BTreeMap<u8, Vec<ResourceValue>>>,
}

fn resolve_resource_label(bytes: &[u8], id: u32) -> Option<String> {
    let table = parse_resource_table(bytes).ok()?;
    resolve_resource(&table, id, 0, &mut BTreeSet::new()).and_then(valid_app_name)
}

fn resolve_resource(
    table: &ResourceTable,
    id: u32,
    depth: usize,
    visiting: &mut BTreeSet<u32>,
) -> Option<String> {
    if depth >= 8 || !visiting.insert(id) {
        return None;
    }
    let variants = table.values.get(&id)?;
    let mut result = None;
    for language in [0, 1, 2] {
        let Some(candidates) = variants.get(&language) else {
            continue;
        };
        let unique = candidates.iter().collect::<BTreeSet<_>>();
        if unique.len() == 1 {
            result = match unique.into_iter().next()? {
                ResourceValue::StringIndex(index) => table.strings.get(*index as usize).cloned(),
                ResourceValue::Reference(next) => {
                    resolve_resource(table, *next, depth + 1, visiting)
                }
            };
        }
        break;
    }
    visiting.remove(&id);
    result
}

fn parse_resource_table(bytes: &[u8]) -> Result<ResourceTable, String> {
    if read_u16(bytes, 0)? != 0x0002 {
        return Err("资源表头无效".to_string());
    }
    let table_size = read_u32(bytes, 4)? as usize;
    if table_size > bytes.len() || table_size < 12 {
        return Err("资源表边界无效".to_string());
    }
    let mut global_strings = Vec::new();
    let mut result = BTreeMap::new();
    let mut total_entries = 0usize;
    let mut offset = read_u16(bytes, 2)? as usize;
    while offset < table_size {
        let (kind, header, size) = chunk(bytes, offset, table_size)?;
        if kind == 0x0001 && global_strings.is_empty() {
            global_strings = decode_string_pool(bytes, offset, header, size)?;
        } else if kind == 0x0200 {
            parse_package(
                bytes,
                offset,
                header,
                size,
                &global_strings,
                &mut result,
                &mut total_entries,
            )?;
        }
        offset = offset.checked_add(size).ok_or("资源表偏移溢出")?;
    }
    Ok(ResourceTable {
        strings: global_strings,
        values: result,
    })
}

fn parse_package(
    bytes: &[u8],
    base: usize,
    header: usize,
    size: usize,
    strings: &[String],
    output: &mut BTreeMap<u32, BTreeMap<u8, Vec<ResourceValue>>>,
    total_entries: &mut usize,
) -> Result<(), String> {
    if header < 284 {
        return Err("资源包头无效".to_string());
    }
    let package_id = read_u32(bytes, base + 8)?;
    let end = base.checked_add(size).ok_or("资源包边界溢出")?;
    let mut offset = base.checked_add(header).ok_or("资源包偏移溢出")?;
    while offset < end {
        let (kind, type_header, chunk_size) = chunk(bytes, offset, end)?;
        if kind == 0x0201 {
            parse_type_chunk(
                bytes,
                offset,
                type_header,
                chunk_size,
                package_id,
                ResourceParseState {
                    strings,
                    output,
                    total_entries,
                },
            )?;
        }
        offset = offset.checked_add(chunk_size).ok_or("资源类型偏移溢出")?;
    }
    Ok(())
}

fn parse_type_chunk(
    bytes: &[u8],
    base: usize,
    header: usize,
    size: usize,
    package_id: u32,
    state: ResourceParseState<'_>,
) -> Result<(), String> {
    if header < 28 {
        return Err("资源类型头无效".to_string());
    }
    let type_id = *bytes.get(base + 8).ok_or("资源类型截断")? as u32;
    let flags = *bytes.get(base + 9).ok_or("资源类型截断")?;
    if flags != 0 {
        return Err("不支持的资源类型索引编码".to_string());
    }
    let entry_count = read_u32(bytes, base + 12)? as usize;
    *state.total_entries = state
        .total_entries
        .checked_add(entry_count)
        .ok_or("资源条目数量溢出")?;
    if *state.total_entries > RESOURCE_ENTRY_COUNT_LIMIT {
        return Err("资源条目数量超过上限".to_string());
    }
    let entries_start = read_u32(bytes, base + 16)? as usize;
    let config_size = read_u32(bytes, base + 20)? as usize;
    if config_size < 12 || 20usize.checked_add(config_size).ok_or("资源配置溢出")? > header {
        return Err("资源配置无效".to_string());
    }
    let language = language_rank(bytes.get(base + 28..base + 32).ok_or("资源语言截断")?);
    let offsets_end = base
        .checked_add(header)
        .and_then(|v| v.checked_add(entry_count.checked_mul(4)?))
        .ok_or("资源索引溢出")?;
    let chunk_end = base.checked_add(size).ok_or("资源块溢出")?;
    if offsets_end > chunk_end || base + entries_start > chunk_end {
        return Err("资源索引越界".to_string());
    }
    for index in 0..entry_count {
        let relative = read_u32(bytes, base + header + index * 4)?;
        if relative == u32::MAX {
            continue;
        }
        let entry = base
            .checked_add(entries_start)
            .and_then(|v| v.checked_add(relative as usize))
            .ok_or("资源条目溢出")?;
        if entry + 16 > chunk_end || read_u16(bytes, entry + 2)? & 0x0001 != 0 {
            continue;
        }
        let value = entry + read_u16(bytes, entry)? as usize;
        if read_u16(bytes, value)? < 8 || value + 8 > chunk_end {
            return Err("资源值越界".to_string());
        }
        let data_type = *bytes.get(value + 3).ok_or("资源值截断")?;
        let data = read_u32(bytes, value + 4)?;
        let parsed = match data_type {
            0x03 => {
                state
                    .strings
                    .get(data as usize)
                    .ok_or("资源字符串索引无效")?;
                ResourceValue::StringIndex(data)
            }
            0x01 => ResourceValue::Reference(data),
            _ => continue,
        };
        let id = (package_id << 24) | (type_id << 16) | index as u32;
        state
            .output
            .entry(id)
            .or_default()
            .entry(language)
            .or_default()
            .push(parsed);
    }
    Ok(())
}

fn language_rank(locale: &[u8]) -> u8 {
    match locale {
        [0, 0, _, _] => 0,
        [b'z', b'h', b'C', b'N'] | [b'z', b'h', 0, 0] => 1,
        [b'e', b'n', _, _] => 2,
        _ => 3,
    }
}

fn chunk(bytes: &[u8], offset: usize, outer_end: usize) -> Result<(u16, usize, usize), String> {
    let kind = read_u16(bytes, offset)?;
    let header = read_u16(bytes, offset + 2)? as usize;
    let size = read_u32(bytes, offset + 4)? as usize;
    let end = offset.checked_add(size).ok_or("资源块溢出")?;
    if header < 8 || size < header || end > outer_end || end > bytes.len() {
        return Err("资源块边界无效".to_string());
    }
    Ok((kind, header, size))
}

fn decode_string_pool(
    bytes: &[u8],
    base: usize,
    header: usize,
    size: usize,
) -> Result<Vec<String>, String> {
    if header < 28 {
        return Err("资源字符串池无效".to_string());
    }
    let count = read_u32(bytes, base + 8)? as usize;
    if count > RESOURCE_STRING_COUNT_LIMIT {
        return Err("资源字符串数量超过上限".to_string());
    }
    let flags = read_u32(bytes, base + 16)?;
    let strings_start = read_u32(bytes, base + 20)? as usize;
    if base + header + count * 4 > base + size || strings_start > size {
        return Err("资源字符串池越界".to_string());
    }
    let mut total_bytes = 0usize;
    (0..count)
        .map(|index| {
            let relative = read_u32(bytes, base + header + index * 4)? as usize;
            let start = base
                .checked_add(strings_start)
                .and_then(|v| v.checked_add(relative))
                .ok_or("资源字符串偏移溢出")?;
            let value = if flags & 0x100 != 0 {
                decode_utf8(bytes, start, base + size)
            } else {
                decode_utf16(bytes, start, base + size)
            }?;
            total_bytes = total_bytes
                .checked_add(value.len())
                .ok_or("资源字符串累计大小溢出")?;
            if total_bytes > RESOURCE_STRING_BYTES_LIMIT {
                return Err("资源字符串累计大小超过上限".to_string());
            }
            Ok(value)
        })
        .collect()
}

fn decode_utf8(bytes: &[u8], offset: usize, end: usize) -> Result<String, String> {
    let (_, next) = length8(bytes, offset, end)?;
    let (len, start) = length8(bytes, next, end)?;
    let slice = bytes
        .get(start..start.checked_add(len).ok_or("资源字符串溢出")?)
        .filter(|_| start + len <= end)
        .ok_or("资源字符串截断")?;
    String::from_utf8(slice.to_vec()).map_err(|_| "资源 UTF-8 无效".to_string())
}
fn length8(bytes: &[u8], offset: usize, end: usize) -> Result<(usize, usize), String> {
    let first = *bytes
        .get(offset)
        .filter(|_| offset < end)
        .ok_or("资源字符串截断")?;
    if first & 0x80 == 0 {
        Ok((first as usize, offset + 1))
    } else {
        let second = *bytes
            .get(offset + 1)
            .filter(|_| offset + 1 < end)
            .ok_or("资源字符串截断")?;
        Ok((((first as usize & 0x7f) << 8) | second as usize, offset + 2))
    }
}
fn decode_utf16(bytes: &[u8], offset: usize, end: usize) -> Result<String, String> {
    let first = read_u16(bytes, offset)?;
    let (len, start) = if first & 0x8000 == 0 {
        (first as usize, offset + 2)
    } else {
        (
            ((first as usize & 0x7fff) << 16) | read_u16(bytes, offset + 2)? as usize,
            offset + 4,
        )
    };
    let byte_len = len.checked_mul(2).ok_or("资源字符串溢出")?;
    if start + byte_len > end {
        return Err("资源字符串截断".to_string());
    }
    let units = bytes[start..start + byte_len]
        .as_chunks::<2>()
        .0
        .iter()
        .map(|p| u16::from_le_bytes([p[0], p[1]]))
        .collect::<Vec<_>>();
    String::from_utf16(&units).map_err(|_| "资源 UTF-16 无效".to_string())
}
fn read_u16(bytes: &[u8], offset: usize) -> Result<u16, String> {
    let v = bytes.get(offset..offset + 2).ok_or("资源数据截断")?;
    Ok(u16::from_le_bytes([v[0], v[1]]))
}
fn read_u32(bytes: &[u8], offset: usize) -> Result<u32, String> {
    let v = bytes.get(offset..offset + 4).ok_or("资源数据截断")?;
    Ok(u32::from_le_bytes([v[0], v[1], v[2], v[3]]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    use std::io::Write;

    #[test]
    fn application_实际解压内容超过限制时即使没有可信声明也会拒绝() {
        let data = vec![0u8; 17];
        assert!(read_limited(Cursor::new(data), "测试条目", 16).is_err());
    }

    #[test]
    fn application_文本清单身份路径失败关闭而不猜测包名() {
        let dir = tempfile::tempdir().expect("应创建临时目录");
        let path = dir.path().join("错误的文件名.apk");
        let file = File::create(&path).expect("应创建测试 APK");
        let mut archive = zip::ZipWriter::new(file);
        archive
            .start_file(
                "AndroidManifest.xml",
                zip::write::SimpleFileOptions::default(),
            )
            .expect("应写入 Manifest");
        archive
            .write_all(r#"<manifest package="com.example.actual" android:versionCode="7"><application android:label="真实名称"/></manifest>"#.as_bytes())
            .expect("应写入 Manifest 内容");
        archive.finish().expect("应完成测试 APK");

        assert!(read_application_identity(&path).is_err());
    }

    #[test]
    fn application_直接名称与版本码可读取() {
        let identity = identity_from_parts(
            "com.example.demo",
            Some(LabelValue::Text("示例应用".to_string())),
            Some(42),
            None,
            None,
        )
        .expect("身份应有效");
        assert_eq!(identity.package_name, "com.example.demo");
        assert_eq!(identity.app_name.as_deref(), Some("示例应用"));
        assert_eq!(identity.app_version_code.as_deref(), Some("42"));
    }

    #[test]
    fn application_资源名称按默认语言简体中文英语回退() {
        let table = test_resource_table(&[
            (0x7f01_0000, "", "默认名称"),
            (0x7f01_0000, "zh-CN", "中文名称"),
            (0x7f01_0000, "en", "English Name"),
            (0x7f01_0001, "zh-CN", "中文回退"),
            (0x7f01_0001, "en", "English Fallback"),
            (0x7f01_0002, "en", "English Only"),
        ]);
        assert_eq!(
            resolve_resource_label(&table, 0x7f01_0000).as_deref(),
            Some("默认名称")
        );
        assert_eq!(
            resolve_resource_label(&table, 0x7f01_0001).as_deref(),
            Some("中文回退")
        );
        assert_eq!(
            resolve_resource_label(&table, 0x7f01_0002).as_deref(),
            Some("English Only")
        );
    }

    #[test]
    fn application_重复字符串偏移超过累计预算时安全失败() {
        let repeated = "x".repeat(100);
        let pool = encode_repeated_offset_string_pool(45_000, &repeated);
        assert!(decode_string_pool(&pool, 0, 28, pool.len()).is_err());
    }

    #[test]
    fn application_不支持的稀疏或十六位偏移资源块不会被误读() {
        for flags in [0x01, 0x02] {
            let mut table = test_resource_table(&[(0x7f01_0000, "", "名称")]);
            let type_offset = find_chunk_offset(&table, 0x0201).expect("应包含资源类型块");
            table[type_offset + 9] = flags;
            assert_eq!(resolve_resource_label(&table, 0x7f01_0000), None);
        }
    }

    #[test]
    fn application_资源字符串和条目计数超过预算时安全失败() {
        let mut table = test_resource_table(&[(0x7f01_0000, "", "名称")]);
        let pool_offset = 12usize;
        put_u32(
            &mut table,
            pool_offset + 8,
            (RESOURCE_STRING_COUNT_LIMIT + 1) as u32,
        );
        assert!(parse_resource_table(&table).is_err());

        let mut table = test_resource_table(&[(0x7f01_0000, "", "名称")]);
        let type_offset = find_chunk_offset(&table, 0x0201).expect("应包含资源类型块");
        put_u32(
            &mut table,
            type_offset + 12,
            (RESOURCE_ENTRY_COUNT_LIMIT + 1) as u32,
        );
        assert!(parse_resource_table(&table).is_err());
    }

    #[test]
    fn application_资源引用循环与超过八层均返回空名称() {
        let cycle = test_reference_table(&[(0x7f01_0000, 0x7f01_0001), (0x7f01_0001, 0x7f01_0000)]);
        assert_eq!(resolve_resource_label(&cycle, 0x7f01_0000), None);
        let deep = test_reference_table(&[
            (0x7f01_0000, 0x7f01_0001),
            (0x7f01_0001, 0x7f01_0002),
            (0x7f01_0002, 0x7f01_0003),
            (0x7f01_0003, 0x7f01_0004),
            (0x7f01_0004, 0x7f01_0005),
            (0x7f01_0005, 0x7f01_0006),
            (0x7f01_0006, 0x7f01_0007),
            (0x7f01_0007, 0x7f01_0008),
            (0x7f01_0008, 0x7f01_0009),
        ]);
        assert_eq!(resolve_resource_label(&deep, 0x7f01_0000), None);
    }

    #[test]
    fn application_名称与版本和包名边界被严格限制() {
        assert!(identity_from_parts("9invalid.package", None, None, None, None).is_err());
        assert!(identity_from_parts("com..invalid", None, None, None, None).is_err());
        let too_large = identity_from_parts(
            "com.example.app",
            None,
            Some(u32::MAX),
            Some(0x8000_0000),
            None,
        )
        .expect("可靠包名仍应保留身份");
        assert_eq!(too_large.app_version_code, None);
        let boundary = identity_from_parts(
            "com.example.app",
            None,
            Some(u32::MAX),
            Some(0x7fff_ffff),
            None,
        )
        .expect("有符号 64 位上限应有效");
        assert_eq!(
            boundary.app_version_code.as_deref(),
            Some("9223372036854775807")
        );
        let invalid_name = identity_from_parts(
            "com.example.app",
            Some(LabelValue::Text("包含\n控制符".to_string())),
            None,
            None,
            None,
        )
        .expect("名称不可靠不应拒绝身份");
        assert_eq!(invalid_name.app_name, None);
    }

    #[test]
    fn application_超大清单和资源文件会被拒绝读取() {
        assert!(validate_entry_size("AndroidManifest.xml", 8 * 1024 * 1024 + 1).is_err());
        assert!(validate_entry_size("resources.arsc", 32 * 1024 * 1024 + 1).is_err());
    }

    fn test_resource_table(values: &[(u32, &str, &str)]) -> Vec<u8> {
        encode_test_resource_table(values, &[])
    }

    fn test_reference_table(values: &[(u32, u32)]) -> Vec<u8> {
        encode_test_resource_table(&[], values)
    }

    fn encode_test_resource_table(
        texts: &[(u32, &str, &str)],
        references: &[(u32, u32)],
    ) -> Vec<u8> {
        let string_values = texts.iter().map(|(_, _, value)| *value).collect::<Vec<_>>();
        let string_pool = encode_string_pool(&string_values);
        let mut variants: BTreeMap<String, Vec<(u32, u8, u32)>> = BTreeMap::new();
        for (index, (id, locale, _)) in texts.iter().enumerate() {
            variants
                .entry((*locale).to_string())
                .or_default()
                .push((*id, 0x03, index as u32));
        }
        for (id, target) in references {
            variants
                .entry(String::new())
                .or_default()
                .push((*id, 0x01, *target));
        }
        let mut package = vec![0; 288];
        put_u16(&mut package, 0, 0x0200);
        put_u16(&mut package, 2, 288);
        put_u32(&mut package, 8, 0x7f);
        for (locale, values) in variants {
            package.extend(encode_type_chunk(&locale, &values));
        }
        let package_len = package.len() as u32;
        put_u32(&mut package, 4, package_len);
        let mut table = vec![0; 12];
        put_u16(&mut table, 0, 0x0002);
        put_u16(&mut table, 2, 12);
        put_u32(&mut table, 8, 1);
        table.extend(string_pool);
        table.extend(package);
        let table_len = table.len() as u32;
        put_u32(&mut table, 4, table_len);
        table
    }

    fn encode_string_pool(strings: &[&str]) -> Vec<u8> {
        let mut data = Vec::new();
        let mut offsets = Vec::new();
        for value in strings {
            offsets.push(data.len() as u32);
            data.push(value.chars().count() as u8);
            data.push(value.len() as u8);
            data.extend(value.as_bytes());
            data.push(0);
        }
        let start = 28 + offsets.len() * 4;
        let mut pool = vec![0; 28];
        put_u16(&mut pool, 0, 0x0001);
        put_u16(&mut pool, 2, 28);
        put_u32(&mut pool, 8, strings.len() as u32);
        put_u32(&mut pool, 16, 0x100);
        put_u32(&mut pool, 20, start as u32);
        for offset in offsets {
            pool.extend(offset.to_le_bytes());
        }
        pool.extend(data);
        let len = pool.len() as u32;
        put_u32(&mut pool, 4, len);
        pool
    }

    fn encode_repeated_offset_string_pool(count: usize, value: &str) -> Vec<u8> {
        let start = 28 + count * 4;
        let mut pool = vec![0; 28];
        put_u16(&mut pool, 0, 0x0001);
        put_u16(&mut pool, 2, 28);
        put_u32(&mut pool, 8, count as u32);
        put_u32(&mut pool, 16, 0x100);
        put_u32(&mut pool, 20, start as u32);
        for _ in 0..count {
            pool.extend(0u32.to_le_bytes());
        }
        pool.push(value.chars().count() as u8);
        pool.push(value.len() as u8);
        pool.extend(value.as_bytes());
        pool.push(0);
        let len = pool.len() as u32;
        put_u32(&mut pool, 4, len);
        pool
    }

    fn encode_type_chunk(locale: &str, values: &[(u32, u8, u32)]) -> Vec<u8> {
        let count = values
            .iter()
            .map(|(id, _, _)| (id & 0xffff) as usize + 1)
            .max()
            .unwrap_or(0);
        let header = 84usize;
        let entries_start = header + count * 4;
        let mut chunk = vec![0; entries_start];
        put_u16(&mut chunk, 0, 0x0201);
        put_u16(&mut chunk, 2, header as u16);
        chunk[8] = 1;
        put_u32(&mut chunk, 12, count as u32);
        put_u32(&mut chunk, 16, entries_start as u32);
        put_u32(&mut chunk, 20, 64);
        match locale {
            "zh-CN" => {
                chunk[28..32].copy_from_slice(b"zhCN");
            }
            "en" => {
                chunk[28..30].copy_from_slice(b"en");
            }
            _ => {}
        }
        for index in 0..count {
            put_u32(&mut chunk, header + index * 4, u32::MAX);
        }
        for (id, kind, data) in values {
            let index = (id & 0xffff) as usize;
            let relative = chunk.len() - entries_start;
            put_u32(&mut chunk, header + index * 4, relative as u32);
            chunk.extend(8u16.to_le_bytes());
            chunk.extend(0u16.to_le_bytes());
            chunk.extend(0u32.to_le_bytes());
            chunk.extend(8u16.to_le_bytes());
            chunk.push(0);
            chunk.push(*kind);
            chunk.extend(data.to_le_bytes());
        }
        let len = chunk.len() as u32;
        put_u32(&mut chunk, 4, len);
        chunk
    }
    fn put_u16(bytes: &mut [u8], offset: usize, value: u16) {
        bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
    }
    fn put_u32(bytes: &mut [u8], offset: usize, value: u32) {
        bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }

    fn find_chunk_offset(bytes: &[u8], kind: u16) -> Option<usize> {
        bytes
            .windows(2)
            .position(|window| window == kind.to_le_bytes())
    }
}

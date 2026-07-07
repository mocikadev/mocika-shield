use anyhow::Result;
use std::fs;
use std::path::Path;

const AROUTER_ROUTES_PREFIX: &str = "com/alibaba/android/arouter/routes/";

/// 扫描 DEX 目录，提取所有 ARouter 路由表类的全限定名。
/// 解析 DEX header 的 class_defs 段，不依赖任何运行时 API。
pub fn scan_arouter_routes(dex_dir: &Path) -> Result<Vec<String>> {
    let mut routes = Vec::new();

    let mut dex_paths: Vec<_> = fs::read_dir(dex_dir)?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|s| s.to_str()) == Some("dex"))
        .collect();
    dex_paths.sort();

    for path in &dex_paths {
        let data = fs::read(path)?;
        for name in parse_dex_class_names(&data)? {
            if name.starts_with(AROUTER_ROUTES_PREFIX) {
                let java_name = name.replace('/', ".");
                routes.push(java_name);
            }
        }
    }

    routes.sort();
    routes.dedup();
    Ok(routes)
}

/// 解析 DEX 文件，提取所有类的描述符（内部格式，如 "Lcom/foo/Bar;"）
/// 转换为斜线分隔路径（如 "com/foo/Bar"）。
///
/// DEX 格式参考：https://source.android.com/docs/core/runtime/dex-format
/// 仅解析 header、string_ids、type_ids、class_defs 四个区段，满足类名提取需求。
fn parse_dex_class_names(data: &[u8]) -> Result<Vec<String>> {
    if data.len() < 112 {
        anyhow::bail!("DEX 文件过小，不是有效的 DEX");
    }
    if &data[0..4] != b"dex\n" {
        anyhow::bail!("Magic 不匹配，不是有效的 DEX 文件");
    }

    let string_ids_size = u32_le(data, 56) as usize;
    let string_ids_off = u32_le(data, 60) as usize;
    let type_ids_size = u32_le(data, 64) as usize;
    let type_ids_off = u32_le(data, 68) as usize;
    let class_defs_size = u32_le(data, 96) as usize;
    let class_defs_off = u32_le(data, 100) as usize;

    let mut names = Vec::with_capacity(class_defs_size);

    for i in 0..class_defs_size {
        let def_off = class_defs_off + i * 32;
        if def_off + 4 > data.len() {
            break;
        }
        let class_idx = u32_le(data, def_off) as usize;
        if class_idx >= type_ids_size {
            continue;
        }

        let type_off = type_ids_off + class_idx * 4;
        if type_off + 4 > data.len() {
            continue;
        }
        let string_idx = u32_le(data, type_off) as usize;
        if string_idx >= string_ids_size {
            continue;
        }

        let sid_off = string_ids_off + string_idx * 4;
        if sid_off + 4 > data.len() {
            continue;
        }
        let string_data_off = u32_le(data, sid_off) as usize;

        if let Some(descriptor) = read_mutf8_string(data, string_data_off) {
            // 描述符格式: "Lcom/foo/Bar;" → 取中间部分 "com/foo/Bar"
            if descriptor.starts_with('L') && descriptor.ends_with(';') {
                names.push(descriptor[1..descriptor.len() - 1].to_string());
            }
        }
    }

    Ok(names)
}

/// 读取 DEX MUTF-8 编码字符串（string_data_item）。
/// 格式：ULEB128 长度前缀 + MUTF-8 字节 + NUL 终止符。
/// ARouter 路由表类名全为 ASCII，简化实现足够。
fn read_mutf8_string(data: &[u8], offset: usize) -> Option<&str> {
    if offset >= data.len() {
        return None;
    }
    // 跳过 ULEB128 长度前缀（每字节高位为 1 则继续，最后一字节高位为 0）
    let mut pos = offset;
    while pos < data.len() && data[pos] & 0x80 != 0 {
        pos += 1;
    }
    if pos >= data.len() {
        return None;
    }
    pos += 1; // 跳过最后一个 ULEB128 字节

    let start = pos;
    while pos < data.len() && data[pos] != 0 {
        pos += 1;
    }
    std::str::from_utf8(&data[start..pos]).ok()
}

#[inline]
fn u32_le(data: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes([
        data[offset],
        data[offset + 1],
        data[offset + 2],
        data[offset + 3],
    ])
}

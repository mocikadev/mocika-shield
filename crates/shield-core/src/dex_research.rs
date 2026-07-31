//! DEX 方法代码分离的宿主端研究入口。
//!
//! 当前模块仅在测试构建中启用，只负责只读解析和方法清点；不得接入正式加固流程。

use thiserror::Error;

mod benchmark;
mod bundle;
mod layered;
mod manifest;
mod metrics;
mod transform;

const HEADER_SIZE: usize = 112;
const METHOD_ID_SIZE: usize = 8;
const PROTO_ID_SIZE: usize = 12;
const CLASS_DEF_SIZE: usize = 32;
const NO_INDEX: u32 = u32::MAX;
const ACC_NATIVE: u32 = 0x0100;
const ACC_ABSTRACT: u32 = 0x0400;

#[derive(Debug, Error, PartialEq, Eq)]
enum DexInventoryError {
    #[error("DEX 文件过短：至少需要 {HEADER_SIZE} 字节，实际 {actual} 字节")]
    HeaderTooShort { actual: usize },
    #[error("DEX magic 或版本格式无效")]
    InvalidMagic,
    #[error("DEX header_size 无效：期望 {HEADER_SIZE}，实际 {actual}")]
    InvalidHeaderSize { actual: u32 },
    #[error("DEX file_size 无效：声明 {declared} 字节，实际 {actual} 字节")]
    InvalidFileSize { declared: usize, actual: usize },
    #[error("DEX endian_tag 不受支持：0x{actual:08x}")]
    UnsupportedEndian { actual: u32 },
    #[error("{section} 区段越界：offset={offset}, size={size}, file_size={file_size}")]
    SectionOutOfBounds {
        section: &'static str,
        offset: usize,
        size: usize,
        file_size: usize,
    },
    #[error("{kind} 索引越界：index={index}, count={count}")]
    IndexOutOfBounds {
        kind: &'static str,
        index: usize,
        count: usize,
    },
    #[error("ULEB128 在 offset={offset} 处无效")]
    InvalidUleb128 { offset: usize },
    #[error("MUTF-8 字符串在 offset={offset} 处无效")]
    InvalidMutf8 { offset: usize },
    #[error("类数据中的方法索引发生溢出")]
    MethodIndexOverflow,
    #[error("方法 {method_index} 的 code_off 未按 4 字节对齐：{code_offset}")]
    UnalignedCodeItem {
        method_index: u32,
        code_offset: usize,
    },
    #[error("Native 或 Abstract 方法 {method_index} 不应包含 code_item")]
    UnexpectedCodeItem { method_index: u32 },
}

type Result<T> = std::result::Result<T, DexInventoryError>;

#[derive(Debug, Clone, PartialEq, Eq)]
struct DexInventory {
    version: String,
    methods: Vec<MethodInventory>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct MethodInventory {
    method_index: u32,
    class_descriptor: String,
    name: String,
    prototype: String,
    access_flags: u32,
    code_offset: Option<u32>,
    code: Option<CodeInventory>,
    disposition: MethodDisposition,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CodeInventory {
    instructions_offset: u32,
    registers_size: u16,
    tries_size: u16,
    debug_info_offset: Option<u32>,
    instructions_size: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MethodDisposition {
    Eligible,
    Native,
    Abstract,
    Constructor,
    StaticInitializer,
    MissingCode,
    InstructionSpaceTooSmall,
    RegistersTooSmall,
}

#[derive(Debug, Clone, Copy)]
struct Header {
    file_size: usize,
    string_ids_size: usize,
    string_ids_offset: usize,
    type_ids_size: usize,
    type_ids_offset: usize,
    proto_ids_size: usize,
    proto_ids_offset: usize,
    method_ids_size: usize,
    method_ids_offset: usize,
    class_defs_size: usize,
    class_defs_offset: usize,
}

#[derive(Debug, Clone)]
struct MethodId {
    class_descriptor: String,
    name: String,
    prototype: String,
    return_descriptor: String,
}

fn inventory_methods(data: &[u8]) -> Result<DexInventory> {
    let (header, version) = parse_header(data)?;
    let mut methods = Vec::new();

    for class_index in 0..header.class_defs_size {
        let class_def_offset = table_item_offset(
            &header,
            header.class_defs_offset,
            class_index,
            CLASS_DEF_SIZE,
            "class_defs",
        )?;
        let class_data_offset = read_u32(data, class_def_offset + 24)? as usize;
        if class_data_offset == 0 {
            continue;
        }
        parse_class_data(data, &header, class_data_offset, &mut methods)?;
    }

    Ok(DexInventory { version, methods })
}

fn parse_header(data: &[u8]) -> Result<(Header, String)> {
    if data.len() < HEADER_SIZE {
        return Err(DexInventoryError::HeaderTooShort { actual: data.len() });
    }
    if &data[0..4] != b"dex\n" || data[7] != 0 || !data[4..7].iter().all(u8::is_ascii_digit) {
        return Err(DexInventoryError::InvalidMagic);
    }

    let file_size = read_u32(data, 32)? as usize;
    if file_size < HEADER_SIZE || file_size > data.len() {
        return Err(DexInventoryError::InvalidFileSize {
            declared: file_size,
            actual: data.len(),
        });
    }
    let header_size = read_u32(data, 36)?;
    if header_size != HEADER_SIZE as u32 {
        return Err(DexInventoryError::InvalidHeaderSize {
            actual: header_size,
        });
    }
    let endian_tag = read_u32(data, 40)?;
    if endian_tag != 0x1234_5678 {
        return Err(DexInventoryError::UnsupportedEndian { actual: endian_tag });
    }

    let header = Header {
        file_size,
        string_ids_size: read_u32(data, 56)? as usize,
        string_ids_offset: read_u32(data, 60)? as usize,
        type_ids_size: read_u32(data, 64)? as usize,
        type_ids_offset: read_u32(data, 68)? as usize,
        proto_ids_size: read_u32(data, 72)? as usize,
        proto_ids_offset: read_u32(data, 76)? as usize,
        method_ids_size: read_u32(data, 88)? as usize,
        method_ids_offset: read_u32(data, 92)? as usize,
        class_defs_size: read_u32(data, 96)? as usize,
        class_defs_offset: read_u32(data, 100)? as usize,
    };

    validate_table(
        &header,
        header.string_ids_offset,
        header.string_ids_size,
        4,
        "string_ids",
    )?;
    validate_table(
        &header,
        header.type_ids_offset,
        header.type_ids_size,
        4,
        "type_ids",
    )?;
    validate_table(
        &header,
        header.proto_ids_offset,
        header.proto_ids_size,
        PROTO_ID_SIZE,
        "proto_ids",
    )?;
    validate_table(
        &header,
        header.method_ids_offset,
        header.method_ids_size,
        METHOD_ID_SIZE,
        "method_ids",
    )?;
    validate_table(
        &header,
        header.class_defs_offset,
        header.class_defs_size,
        CLASS_DEF_SIZE,
        "class_defs",
    )?;

    let version = std::str::from_utf8(&data[4..7])
        .map_err(|_| DexInventoryError::InvalidMagic)?
        .to_string();
    Ok((header, version))
}

fn parse_class_data(
    data: &[u8],
    header: &Header,
    offset: usize,
    output: &mut Vec<MethodInventory>,
) -> Result<()> {
    ensure_range(header, offset, 1, "class_data")?;
    let mut cursor = offset;
    let static_fields = read_uleb128(data, header, &mut cursor)? as usize;
    let instance_fields = read_uleb128(data, header, &mut cursor)? as usize;
    let direct_methods = read_uleb128(data, header, &mut cursor)? as usize;
    let virtual_methods = read_uleb128(data, header, &mut cursor)? as usize;

    for _ in 0..static_fields.saturating_add(instance_fields) {
        read_uleb128(data, header, &mut cursor)?;
        read_uleb128(data, header, &mut cursor)?;
    }
    parse_encoded_methods(data, header, &mut cursor, direct_methods, output)?;
    parse_encoded_methods(data, header, &mut cursor, virtual_methods, output)
}

fn parse_encoded_methods(
    data: &[u8],
    header: &Header,
    cursor: &mut usize,
    count: usize,
    output: &mut Vec<MethodInventory>,
) -> Result<()> {
    let mut method_index = 0u32;
    for _ in 0..count {
        method_index = method_index
            .checked_add(read_uleb128(data, header, cursor)?)
            .ok_or(DexInventoryError::MethodIndexOverflow)?;
        let access_flags = read_uleb128(data, header, cursor)?;
        let code_offset = read_uleb128(data, header, cursor)?;
        output.push(inventory_method(
            data,
            header,
            method_index,
            access_flags,
            code_offset,
        )?);
    }
    Ok(())
}

fn inventory_method(
    data: &[u8],
    header: &Header,
    method_index: u32,
    access_flags: u32,
    code_offset: u32,
) -> Result<MethodInventory> {
    let method = parse_method_id(data, header, method_index as usize)?;
    let is_native = access_flags & ACC_NATIVE != 0;
    let is_abstract = access_flags & ACC_ABSTRACT != 0;

    if (is_native || is_abstract) && code_offset != 0 {
        return Err(DexInventoryError::UnexpectedCodeItem { method_index });
    }
    let code = if code_offset == 0 {
        None
    } else {
        Some(parse_code_item(
            data,
            header,
            method_index,
            code_offset as usize,
        )?)
    };

    let disposition = if is_native {
        MethodDisposition::Native
    } else if is_abstract {
        MethodDisposition::Abstract
    } else if method.name == "<init>" {
        MethodDisposition::Constructor
    } else if method.name == "<clinit>" {
        MethodDisposition::StaticInitializer
    } else {
        match &code {
            Some(code) => disposition_for_code(code, &method.return_descriptor),
            None => MethodDisposition::MissingCode,
        }
    };

    Ok(MethodInventory {
        method_index,
        class_descriptor: method.class_descriptor,
        name: method.name,
        prototype: method.prototype,
        access_flags,
        code_offset: (code_offset != 0).then_some(code_offset),
        code,
        disposition,
    })
}

fn disposition_for_code(code: &CodeInventory, return_descriptor: &str) -> MethodDisposition {
    let (required_instructions, required_registers) = match return_descriptor.as_bytes().first() {
        Some(b'V') => (1, 0),
        Some(b'J' | b'D') => (3, 2),
        _ => (2, 1),
    };
    if code.instructions_size < required_instructions {
        MethodDisposition::InstructionSpaceTooSmall
    } else if code.registers_size < required_registers {
        MethodDisposition::RegistersTooSmall
    } else {
        MethodDisposition::Eligible
    }
}

fn parse_method_id(data: &[u8], header: &Header, index: usize) -> Result<MethodId> {
    if index >= header.method_ids_size {
        return Err(DexInventoryError::IndexOutOfBounds {
            kind: "method_id",
            index,
            count: header.method_ids_size,
        });
    }
    let offset = table_item_offset(
        header,
        header.method_ids_offset,
        index,
        METHOD_ID_SIZE,
        "method_ids",
    )?;
    let class_index = read_u16(data, offset)? as usize;
    let proto_index = read_u16(data, offset + 2)? as usize;
    let name_index = read_u32(data, offset + 4)? as usize;
    let class_descriptor = read_type_descriptor(data, header, class_index)?;
    let name = read_string(data, header, name_index)?;
    let (prototype, return_descriptor) = read_prototype(data, header, proto_index)?;
    Ok(MethodId {
        class_descriptor,
        name,
        prototype,
        return_descriptor,
    })
}

fn read_prototype(data: &[u8], header: &Header, index: usize) -> Result<(String, String)> {
    if index >= header.proto_ids_size {
        return Err(DexInventoryError::IndexOutOfBounds {
            kind: "proto_id",
            index,
            count: header.proto_ids_size,
        });
    }
    let offset = table_item_offset(
        header,
        header.proto_ids_offset,
        index,
        PROTO_ID_SIZE,
        "proto_ids",
    )?;
    let return_descriptor =
        read_type_descriptor(data, header, read_u32(data, offset + 4)? as usize)?;
    let parameters_offset = read_u32(data, offset + 8)? as usize;
    let mut prototype = String::from("(");
    if parameters_offset != 0 {
        ensure_range(header, parameters_offset, 4, "type_list")?;
        let parameter_count = read_u32(data, parameters_offset)? as usize;
        let byte_count =
            parameter_count
                .checked_mul(2)
                .ok_or(DexInventoryError::SectionOutOfBounds {
                    section: "type_list",
                    offset: parameters_offset,
                    size: usize::MAX,
                    file_size: header.file_size,
                })?;
        ensure_range(header, parameters_offset + 4, byte_count, "type_list")?;
        for parameter_index in 0..parameter_count {
            let type_index = read_u16(data, parameters_offset + 4 + parameter_index * 2)? as usize;
            prototype.push_str(&read_type_descriptor(data, header, type_index)?);
        }
    }
    prototype.push(')');
    prototype.push_str(&return_descriptor);
    Ok((prototype, return_descriptor))
}

fn read_type_descriptor(data: &[u8], header: &Header, index: usize) -> Result<String> {
    if index >= header.type_ids_size {
        return Err(DexInventoryError::IndexOutOfBounds {
            kind: "type_id",
            index,
            count: header.type_ids_size,
        });
    }
    let offset = table_item_offset(header, header.type_ids_offset, index, 4, "type_ids")?;
    read_string(data, header, read_u32(data, offset)? as usize)
}

fn read_string(data: &[u8], header: &Header, index: usize) -> Result<String> {
    if index >= header.string_ids_size {
        return Err(DexInventoryError::IndexOutOfBounds {
            kind: "string_id",
            index,
            count: header.string_ids_size,
        });
    }
    let item_offset = table_item_offset(header, header.string_ids_offset, index, 4, "string_ids")?;
    let string_offset = read_u32(data, item_offset)? as usize;
    ensure_range(header, string_offset, 1, "string_data")?;
    let mut cursor = string_offset;
    let expected_utf16_length = read_uleb128(data, header, &mut cursor)? as usize;
    let mut units = Vec::with_capacity(expected_utf16_length);

    while cursor < header.file_size {
        let first = data[cursor];
        cursor += 1;
        if first == 0 {
            let decoded =
                String::from_utf16(&units).map_err(|_| DexInventoryError::InvalidMutf8 {
                    offset: string_offset,
                })?;
            if decoded.encode_utf16().count() != expected_utf16_length {
                return Err(DexInventoryError::InvalidMutf8 {
                    offset: string_offset,
                });
            }
            return Ok(decoded);
        }
        let unit = if first & 0x80 == 0 {
            first as u16
        } else if first & 0xe0 == 0xc0 {
            ensure_range(header, cursor, 1, "string_data")?;
            let second = data[cursor];
            cursor += 1;
            if second & 0xc0 != 0x80 {
                return Err(DexInventoryError::InvalidMutf8 {
                    offset: string_offset,
                });
            }
            (((first & 0x1f) as u16) << 6) | (second & 0x3f) as u16
        } else if first & 0xf0 == 0xe0 {
            ensure_range(header, cursor, 2, "string_data")?;
            let second = data[cursor];
            let third = data[cursor + 1];
            cursor += 2;
            if second & 0xc0 != 0x80 || third & 0xc0 != 0x80 {
                return Err(DexInventoryError::InvalidMutf8 {
                    offset: string_offset,
                });
            }
            (((first & 0x0f) as u16) << 12)
                | (((second & 0x3f) as u16) << 6)
                | (third & 0x3f) as u16
        } else {
            return Err(DexInventoryError::InvalidMutf8 {
                offset: string_offset,
            });
        };
        units.push(unit);
    }
    Err(DexInventoryError::InvalidMutf8 {
        offset: string_offset,
    })
}

fn parse_code_item(
    data: &[u8],
    header: &Header,
    method_index: u32,
    offset: usize,
) -> Result<CodeInventory> {
    if !offset.is_multiple_of(4) {
        return Err(DexInventoryError::UnalignedCodeItem {
            method_index,
            code_offset: offset,
        });
    }
    ensure_range(header, offset, 16, "code_item")?;
    let registers_size = read_u16(data, offset)?;
    let tries_size = read_u16(data, offset + 6)?;
    let debug_info_offset = read_u32(data, offset + 8)?;
    let instructions_size = read_u32(data, offset + 12)?;
    let instruction_bytes = (instructions_size as usize).checked_mul(2).ok_or(
        DexInventoryError::SectionOutOfBounds {
            section: "code_item insns",
            offset: offset + 16,
            size: usize::MAX,
            file_size: header.file_size,
        },
    )?;
    ensure_range(header, offset + 16, instruction_bytes, "code_item insns")?;

    if tries_size != 0 {
        let padding = if instructions_size % 2 == 0 { 0 } else { 2 };
        let tries_offset = offset
            .checked_add(16)
            .and_then(|value| value.checked_add(instruction_bytes))
            .and_then(|value| value.checked_add(padding))
            .ok_or(DexInventoryError::SectionOutOfBounds {
                section: "try_items",
                offset,
                size: usize::MAX,
                file_size: header.file_size,
            })?;
        let tries_bytes =
            (tries_size as usize)
                .checked_mul(8)
                .ok_or(DexInventoryError::SectionOutOfBounds {
                    section: "try_items",
                    offset: tries_offset,
                    size: usize::MAX,
                    file_size: header.file_size,
                })?;
        ensure_range(header, tries_offset, tries_bytes, "try_items")?;
    }
    if debug_info_offset != 0 {
        ensure_range(header, debug_info_offset as usize, 1, "debug_info")?;
    }

    Ok(CodeInventory {
        instructions_offset: (offset + 16) as u32,
        registers_size,
        tries_size,
        debug_info_offset: (debug_info_offset != 0).then_some(debug_info_offset),
        instructions_size,
    })
}

fn validate_table(
    header: &Header,
    offset: usize,
    count: usize,
    item_size: usize,
    section: &'static str,
) -> Result<()> {
    if count == 0 {
        return Ok(());
    }
    let size = count
        .checked_mul(item_size)
        .ok_or(DexInventoryError::SectionOutOfBounds {
            section,
            offset,
            size: usize::MAX,
            file_size: header.file_size,
        })?;
    ensure_range(header, offset, size, section)
}

fn table_item_offset(
    header: &Header,
    table_offset: usize,
    index: usize,
    item_size: usize,
    section: &'static str,
) -> Result<usize> {
    let relative = index
        .checked_mul(item_size)
        .ok_or(DexInventoryError::SectionOutOfBounds {
            section,
            offset: table_offset,
            size: usize::MAX,
            file_size: header.file_size,
        })?;
    let offset =
        table_offset
            .checked_add(relative)
            .ok_or(DexInventoryError::SectionOutOfBounds {
                section,
                offset: table_offset,
                size: usize::MAX,
                file_size: header.file_size,
            })?;
    ensure_range(header, offset, item_size, section)?;
    Ok(offset)
}

fn ensure_range(header: &Header, offset: usize, size: usize, section: &'static str) -> Result<()> {
    let end = offset
        .checked_add(size)
        .ok_or(DexInventoryError::SectionOutOfBounds {
            section,
            offset,
            size,
            file_size: header.file_size,
        })?;
    if end > header.file_size {
        return Err(DexInventoryError::SectionOutOfBounds {
            section,
            offset,
            size,
            file_size: header.file_size,
        });
    }
    Ok(())
}

fn read_u16(data: &[u8], offset: usize) -> Result<u16> {
    let bytes = data
        .get(offset..offset + 2)
        .ok_or(DexInventoryError::SectionOutOfBounds {
            section: "u16",
            offset,
            size: 2,
            file_size: data.len(),
        })?;
    Ok(u16::from_le_bytes(bytes.try_into().unwrap()))
}

fn read_u32(data: &[u8], offset: usize) -> Result<u32> {
    let bytes = data
        .get(offset..offset + 4)
        .ok_or(DexInventoryError::SectionOutOfBounds {
            section: "u32",
            offset,
            size: 4,
            file_size: data.len(),
        })?;
    Ok(u32::from_le_bytes(bytes.try_into().unwrap()))
}

fn read_uleb128(data: &[u8], header: &Header, cursor: &mut usize) -> Result<u32> {
    let start = *cursor;
    let mut value = 0u32;
    for shift in (0..35).step_by(7) {
        ensure_range(header, *cursor, 1, "ULEB128")?;
        let byte = data[*cursor];
        *cursor += 1;
        if shift == 28 && byte & 0xf0 != 0 {
            return Err(DexInventoryError::InvalidUleb128 { offset: start });
        }
        value |= ((byte & 0x7f) as u32) << shift;
        if byte & 0x80 == 0 {
            return Ok(value);
        }
    }
    Err(DexInventoryError::InvalidUleb128 { offset: start })
}

#[cfg(test)]
mod tests {
    use super::{
        bundle::{open_bundle, seal_bundle, BundleCarrier, BundleError},
        inventory_methods,
        layered::{open_layered_each, seal_layered, LayeredError},
        manifest::{restore_group, seal_group, GroupManifestError, GroupMember},
        transform::{
            extract_and_stub, extract_and_stub_selected, make_stub, repair_header, restore,
        },
        DexInventoryError, MethodDisposition, HEADER_SIZE, NO_INDEX,
    };

    #[test]
    fn 清点构造普通与_native_方法() {
        let dex = make_test_dex();
        let inventory = inventory_methods(&dex).unwrap();

        assert_eq!(inventory.version, "035");
        assert_eq!(inventory.methods.len(), 3);
        assert_eq!(inventory.methods[0].name, "<init>");
        assert_eq!(
            inventory.methods[0].disposition,
            MethodDisposition::Constructor
        );
        assert_eq!(inventory.methods[1].name, "nativeCall");
        assert_eq!(inventory.methods[1].disposition, MethodDisposition::Native);
        assert_eq!(inventory.methods[2].name, "answer");
        assert_eq!(inventory.methods[2].prototype, "()I");
        assert_eq!(
            inventory.methods[2].disposition,
            MethodDisposition::Eligible
        );
        assert_eq!(
            inventory.methods[2]
                .code
                .as_ref()
                .unwrap()
                .instructions_size,
            2
        );
    }

    #[test]
    fn 相同输入生成稳定清单() {
        let dex = make_test_dex();
        assert_eq!(
            inventory_methods(&dex).unwrap(),
            inventory_methods(&dex).unwrap()
        );
    }

    #[test]
    fn 拒绝截断的表区段() {
        let mut dex = make_test_dex();
        let declared = dex.len() as u32;
        dex[32..36].copy_from_slice(&declared.to_le_bytes());
        let invalid_class_defs_offset = dex.len() as u32 - 8;
        dex[100..104].copy_from_slice(&invalid_class_defs_offset.to_le_bytes());

        assert!(matches!(
            inventory_methods(&dex),
            Err(DexInventoryError::SectionOutOfBounds {
                section: "class_defs",
                ..
            })
        ));
    }

    #[test]
    fn 拒绝未对齐的_code_item() {
        let mut dex = make_test_dex();
        let class_data_offset = u32::from_le_bytes(dex[220..224].try_into().unwrap()) as usize;
        let answer_code_uleb_offset = class_data_offset + 16;
        assert!(dex[answer_code_uleb_offset] & 0x80 != 0);
        dex[answer_code_uleb_offset] = dex[answer_code_uleb_offset].wrapping_add(1);

        assert!(matches!(
            inventory_methods(&dex),
            Err(DexInventoryError::UnalignedCodeItem { .. })
        ));
    }

    #[test]
    fn 普通方法可以离线抽取并完整恢复() {
        let mut original = make_test_dex();
        repair_header(&mut original).unwrap();

        let extracted = extract_and_stub(&original).unwrap();
        assert_ne!(extracted.carrier, original);
        assert_eq!(extracted.payload.methods.len(), 1);
        assert_eq!(extracted.payload.methods[0].method_index, 1);
        assert_eq!(extracted.payload.methods[0].original, [0x12, 0x10, 0x0f, 0]);
        assert_eq!(extracted.payload.methods[0].stub, [0x12, 0, 0x0f, 0]);

        let restored = restore(&extracted.carrier, &extracted.payload).unwrap();
        assert_eq!(restored, original);
    }

    #[test]
    fn 恢复拒绝被修改的占位载体() {
        let mut original = make_test_dex();
        repair_header(&mut original).unwrap();
        let extracted = extract_and_stub(&original).unwrap();
        let mut modified = extracted.carrier.clone();
        let offset = extracted.payload.methods[0].instructions_offset as usize;
        modified[offset] ^= 0x01;

        assert!(restore(&modified, &extracted.payload).is_err());
    }

    #[test]
    fn 构造与_native_方法不会进入代码载荷() {
        let mut original = make_test_dex();
        repair_header(&mut original).unwrap();
        let extracted = extract_and_stub(&original).unwrap();

        assert_eq!(extracted.payload.methods.len(), 1);
        assert_eq!(extracted.payload.methods[0].method_index, 1);
        let inventory = inventory_methods(&extracted.carrier).unwrap();
        assert_eq!(
            inventory.methods[0].disposition,
            MethodDisposition::Constructor
        );
        assert_eq!(inventory.methods[1].disposition, MethodDisposition::Native);
    }

    #[test]
    fn 为各类返回值生成类型正确的占位指令() {
        assert_eq!(make_stub(0, "()V", 2).unwrap(), [0x0e, 0]);
        assert_eq!(make_stub(0, "()I", 4).unwrap(), [0x12, 0, 0x0f, 0]);
        assert_eq!(
            make_stub(0, "()Ljava/lang/String;", 4).unwrap(),
            [0x12, 0, 0x11, 0]
        );
        assert_eq!(make_stub(0, "()[I", 4).unwrap(), [0x12, 0, 0x11, 0]);
        assert_eq!(make_stub(0, "()J", 6).unwrap(), [0x16, 0, 0, 0, 0x10, 0]);
    }

    #[test]
    fn 类选择只抽取指定业务类() {
        let mut original = make_test_dex();
        repair_header(&mut original).unwrap();

        let selected = extract_and_stub_selected(&original, &["Lexample/Test;"]).unwrap();
        let skipped = extract_and_stub_selected(&original, &["Lexample/Other;"]).unwrap();
        assert_eq!(selected.payload.methods.len(), 1);
        assert!(skipped.payload.methods.is_empty());
        assert_eq!(skipped.carrier, original);
    }

    #[test]
    fn 结构相同的多个_dex_需要额外身份绑定() {
        let mut first = make_test_dex();
        repair_header(&mut first).unwrap();
        let first_extraction = extract_and_stub(&first).unwrap();

        let mut second = first.clone();
        let offset = first_extraction.payload.methods[0].instructions_offset as usize;
        second[offset] = 0x22;
        repair_header(&mut second).unwrap();
        let second_extraction = extract_and_stub(&second).unwrap();

        assert_eq!(
            restore(&first_extraction.carrier, &first_extraction.payload).unwrap(),
            first
        );
        assert_eq!(
            restore(&second_extraction.carrier, &second_extraction.payload).unwrap(),
            second
        );
        assert_ne!(first, second);
        assert_eq!(first_extraction.carrier, second_extraction.carrier);
        assert_eq!(
            restore(&second_extraction.carrier, &first_extraction.payload).unwrap(),
            first
        );
    }

    #[test]
    fn 认证清单绑定多_dex_身份顺序与载荷() {
        let key = [0x5au8; 32];
        let nonce = [0x31u8; 12];
        let (first, first_extraction, second, second_extraction) = make_group_fixture();
        let members = [
            GroupMember::new("classes.dex", &first, &first_extraction),
            GroupMember::new("classes2.dex", &second, &second_extraction),
        ];

        let sealed = seal_group(&key, nonce, &members).unwrap();
        let repeated = seal_group(&key, nonce, &members).unwrap();
        assert_eq!(sealed, repeated);
        assert_eq!(
            restore_group(&key, &sealed, &members).unwrap(),
            vec![first, second]
        );
    }

    #[test]
    fn 认证清单拒绝多_dex_错序缺失新增与重复身份() {
        let key = [0x5au8; 32];
        let nonce = [0x31u8; 12];
        let (first, first_extraction, second, second_extraction) = make_group_fixture();
        let members = [
            GroupMember::new("classes.dex", &first, &first_extraction),
            GroupMember::new("classes2.dex", &second, &second_extraction),
        ];
        let sealed = seal_group(&key, nonce, &members).unwrap();

        let reversed = [
            GroupMember::new("classes2.dex", &second, &second_extraction),
            GroupMember::new("classes.dex", &first, &first_extraction),
        ];
        assert_eq!(
            restore_group(&key, &sealed, &reversed),
            Err(GroupManifestError::GroupMismatch)
        );
        assert_eq!(
            restore_group(&key, &sealed, &members[..1]),
            Err(GroupManifestError::GroupMismatch)
        );

        let added = [
            GroupMember::new("classes.dex", &first, &first_extraction),
            GroupMember::new("classes2.dex", &second, &second_extraction),
            GroupMember::new("classes3.dex", &first, &first_extraction),
        ];
        assert_eq!(
            restore_group(&key, &sealed, &added),
            Err(GroupManifestError::GroupMismatch)
        );

        let duplicated = [
            GroupMember::new("classes.dex", &first, &first_extraction),
            GroupMember::new("classes.dex", &second, &second_extraction),
        ];
        assert!(matches!(
            seal_group(&key, nonce, &duplicated),
            Err(GroupManifestError::DuplicateIdentity { .. })
        ));
    }

    #[test]
    fn 认证清单拒绝密钥密文载体与代码载荷篡改() {
        let key = [0x5au8; 32];
        let nonce = [0x31u8; 12];
        let (first, first_extraction, second, second_extraction) = make_group_fixture();
        let members = [
            GroupMember::new("classes.dex", &first, &first_extraction),
            GroupMember::new("classes2.dex", &second, &second_extraction),
        ];
        let sealed = seal_group(&key, nonce, &members).unwrap();

        assert_eq!(
            restore_group(&[0x6bu8; 32], &sealed, &members),
            Err(GroupManifestError::AuthenticationFailed)
        );
        let mut tampered_sealed = sealed.clone();
        tampered_sealed.ciphertext[0] ^= 1;
        assert_eq!(
            restore_group(&key, &tampered_sealed, &members),
            Err(GroupManifestError::AuthenticationFailed)
        );

        let mut tampered_carrier = first_extraction.clone();
        tampered_carrier.carrier[8] ^= 1;
        let carrier_members = [
            GroupMember::new("classes.dex", &first, &tampered_carrier),
            GroupMember::new("classes2.dex", &second, &second_extraction),
        ];
        assert_eq!(
            restore_group(&key, &sealed, &carrier_members),
            Err(GroupManifestError::GroupMismatch)
        );

        let mut tampered_payload = first_extraction.clone();
        tampered_payload.payload.methods[0].original[0] ^= 1;
        let payload_members = [
            GroupMember::new("classes.dex", &first, &tampered_payload),
            GroupMember::new("classes2.dex", &second, &second_extraction),
        ];
        assert_eq!(
            restore_group(&key, &sealed, &payload_members),
            Err(GroupManifestError::GroupMismatch)
        );

        let swapped_payloads = [
            GroupMember::new("classes.dex", &first, &second_extraction),
            GroupMember::new("classes2.dex", &second, &first_extraction),
        ];
        assert_eq!(
            restore_group(&key, &sealed, &swapped_payloads),
            Err(GroupManifestError::GroupMismatch)
        );
        assert!(matches!(
            seal_group(&key, nonce, &swapped_payloads),
            Err(GroupManifestError::OriginalMismatch { .. })
        ));
    }

    #[test]
    fn 加密代码载荷整组封装可以恢复双_dex() {
        let key = [0x72u8; 32];
        let nonce = [0x19u8; 12];
        let (first, first_extraction, second, second_extraction) = make_group_fixture();
        let members = [
            GroupMember::new("classes.dex", &first, &first_extraction),
            GroupMember::new("classes2.dex", &second, &second_extraction),
        ];
        let bundle = seal_bundle(&key, nonce, &members).unwrap();
        let carriers = [
            BundleCarrier::new("classes.dex", &first_extraction.carrier),
            BundleCarrier::new("classes2.dex", &second_extraction.carrier),
        ];

        assert_eq!(
            open_bundle(&key, &bundle, &carriers).unwrap(),
            vec![first, second]
        );
    }

    #[test]
    fn 加密代码载荷整组封装拒绝认证失败与载体集合变化() {
        let key = [0x72u8; 32];
        let nonce = [0x19u8; 12];
        let (first, first_extraction, second, second_extraction) = make_group_fixture();
        let members = [
            GroupMember::new("classes.dex", &first, &first_extraction),
            GroupMember::new("classes2.dex", &second, &second_extraction),
        ];
        let bundle = seal_bundle(&key, nonce, &members).unwrap();
        let carriers = [
            BundleCarrier::new("classes.dex", &first_extraction.carrier),
            BundleCarrier::new("classes2.dex", &second_extraction.carrier),
        ];

        assert_eq!(
            open_bundle(&[0x73u8; 32], &bundle, &carriers),
            Err(BundleError::AuthenticationFailed)
        );
        let mut tampered = bundle.clone();
        tampered.ciphertext[0] ^= 1;
        assert_eq!(
            open_bundle(&key, &tampered, &carriers),
            Err(BundleError::AuthenticationFailed)
        );

        let reversed = [
            BundleCarrier::new("classes2.dex", &second_extraction.carrier),
            BundleCarrier::new("classes.dex", &first_extraction.carrier),
        ];
        assert!(matches!(
            open_bundle(&key, &bundle, &reversed),
            Err(BundleError::CarrierIdentityMismatch { .. })
        ));
        assert_eq!(
            open_bundle(&key, &bundle, &carriers[..1]),
            Err(BundleError::CarrierCountMismatch {
                expected: 2,
                actual: 1
            })
        );
    }

    #[test]
    fn 分层认证索引可以按顺序逐_dex_消费() {
        let key = [0x41u8; 32];
        let nonce_base = [0x52u8; 12];
        let (first, first_extraction, second, second_extraction) = make_group_fixture();
        let members = [
            GroupMember::new("classes.dex", &first, &first_extraction),
            GroupMember::new("classes2.dex", &second, &second_extraction),
        ];
        let package = seal_layered(&key, nonce_base, &members).unwrap();
        let carriers = [
            BundleCarrier::new("classes.dex", &first_extraction.carrier),
            BundleCarrier::new("classes2.dex", &second_extraction.carrier),
        ];
        let mut restored = Vec::new();
        open_layered_each(&key, &package, &carriers, |identity, dex| {
            restored.push((identity.to_owned(), dex));
            Ok(())
        })
        .unwrap();

        assert_eq!(restored[0], ("classes.dex".to_owned(), first));
        assert_eq!(restored[1], ("classes2.dex".to_owned(), second));
        assert_ne!(package.index_nonce, package.shards[0].nonce);
        assert_ne!(package.shards[0].nonce, package.shards[1].nonce);
    }

    #[test]
    fn 分层认证索引拒绝缺失错序替换与索引篡改() {
        let key = [0x41u8; 32];
        let (first, first_extraction, second, second_extraction) = make_group_fixture();
        let members = [
            GroupMember::new("classes.dex", &first, &first_extraction),
            GroupMember::new("classes2.dex", &second, &second_extraction),
        ];
        let package = seal_layered(&key, [0x52u8; 12], &members).unwrap();
        let carriers = [
            BundleCarrier::new("classes.dex", &first_extraction.carrier),
            BundleCarrier::new("classes2.dex", &second_extraction.carrier),
        ];

        let mut missing = package.clone();
        missing.shards.pop();
        assert!(matches!(
            open_layered_each(&key, &missing, &carriers, |_, _| Ok(())),
            Err(LayeredError::CountMismatch { .. })
        ));

        let mut reversed = package.clone();
        reversed.shards.reverse();
        assert_eq!(
            open_layered_each(&key, &reversed, &carriers, |_, _| Ok(())),
            Err(LayeredError::IndexMismatch)
        );

        let reversed_carriers = [
            BundleCarrier::new("classes2.dex", &second_extraction.carrier),
            BundleCarrier::new("classes.dex", &first_extraction.carrier),
        ];
        assert_eq!(
            open_layered_each(&key, &package, &reversed_carriers, |_, _| Ok(())),
            Err(LayeredError::IndexMismatch)
        );

        let mut tampered_shard = package.clone();
        tampered_shard.shards[0].ciphertext[0] ^= 1;
        assert_eq!(
            open_layered_each(&key, &tampered_shard, &carriers, |_, _| Ok(())),
            Err(LayeredError::IndexMismatch)
        );

        let mut tampered_index = package.clone();
        tampered_index.index_ciphertext[0] ^= 1;
        assert_eq!(
            open_layered_each(&key, &tampered_index, &carriers, |_, _| Ok(())),
            Err(LayeredError::IndexAuthenticationFailed)
        );

        let mut duplicate_nonce = package.clone();
        duplicate_nonce.shards[1].nonce = duplicate_nonce.shards[0].nonce;
        assert_eq!(
            open_layered_each(&key, &duplicate_nonce, &carriers, |_, _| Ok(())),
            Err(LayeredError::DuplicateNonce)
        );

        let other_package = seal_layered(&key, [0x63u8; 12], &members).unwrap();
        let mut replaced = package.clone();
        replaced.shards[0] = other_package.shards[0].clone();
        assert_eq!(
            open_layered_each(&key, &replaced, &carriers, |_, _| Ok(())),
            Err(LayeredError::IndexMismatch)
        );
    }

    fn make_group_fixture() -> (
        Vec<u8>,
        super::transform::Extraction,
        Vec<u8>,
        super::transform::Extraction,
    ) {
        let mut first = make_test_dex();
        repair_header(&mut first).unwrap();
        let first_extraction = extract_and_stub(&first).unwrap();

        let mut second = first.clone();
        let offset = first_extraction.payload.methods[0].instructions_offset as usize;
        second[offset] = 0x22;
        repair_header(&mut second).unwrap();
        let second_extraction = extract_and_stub(&second).unwrap();
        (first, first_extraction, second, second_extraction)
    }

    #[test]
    #[ignore = "通过环境变量显式生成 Android 语义探针的占位与恢复 DEX"]
    fn 生成_android_语义探针_dex() {
        let input = std::env::var("MOCIKA_DEX_RESEARCH_INPUT").unwrap();
        let output_dir =
            std::path::PathBuf::from(std::env::var("MOCIKA_DEX_RESEARCH_OUTPUT_DIR").unwrap());
        let original = std::fs::read(input).unwrap();
        let configured_classes = std::env::var("MOCIKA_DEX_RESEARCH_CLASSES")
            .unwrap_or_else(|_| "Ldev/mocika/shield/smoke/DexSeparationCases;".to_owned());
        let classes = configured_classes.split(',').collect::<Vec<_>>();
        let extracted = extract_and_stub_selected(&original, &classes).unwrap();
        let restored = restore(&extracted.carrier, &extracted.payload).unwrap();
        assert_eq!(restored, original);
        assert!(!extracted.payload.methods.is_empty());
        std::fs::create_dir_all(&output_dir).unwrap();
        std::fs::write(output_dir.join("carrier.dex"), &extracted.carrier).unwrap();
        std::fs::write(output_dir.join("restored.dex"), &restored).unwrap();
        println!("抽取方法数量：{}", extracted.payload.methods.len());
    }

    #[test]
    #[ignore = "通过环境变量显式评估 DEX 静态保护收益"]
    fn 评估_android_dex_静态收益() {
        let input_dir =
            std::path::PathBuf::from(std::env::var("MOCIKA_DEX_RESEARCH_INPUT_DIR").unwrap());
        let output_dir = std::env::var("MOCIKA_DEX_RESEARCH_OUTPUT_DIR")
            .ok()
            .map(std::path::PathBuf::from);
        if let Some(output_dir) = &output_dir {
            std::fs::create_dir_all(output_dir).unwrap();
        }

        let mut dex_files = std::fs::read_dir(input_dir)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| dex_sequence(path).is_some())
            .collect::<Vec<_>>();
        dex_files.sort_by_key(|path| dex_sequence(path).unwrap());
        assert!(!dex_files.is_empty());

        println!(
            "DEX\t方法总数\t含代码方法\t候选方法\t候选方法占比\t指令字节\t候选指令字节\t候选指令占比\t构造\t静态初始化\tNative\tAbstract\t其他跳过"
        );
        let mut total = super::metrics::DexResearchSummary::default();
        for path in dex_files {
            let data = std::fs::read(&path).unwrap();
            let inventory = inventory_methods(&data).unwrap();
            let summary = super::metrics::summarize(&inventory);
            total.merge(&summary);
            println!(
                "{}\t{}",
                path.file_name().unwrap().to_string_lossy(),
                summary
            );
            if let Some(output_dir) = &output_dir {
                let extracted = extract_and_stub(&data).unwrap();
                std::fs::write(
                    output_dir.join(path.file_name().unwrap()),
                    extracted.carrier,
                )
                .unwrap();
            }
        }
        println!("合计\t{total}");
    }

    #[test]
    #[ignore = "通过环境变量显式评估真实多 DEX 封装资源成本"]
    fn 评估真实多_dex_封装资源成本() {
        let input_dir =
            std::path::PathBuf::from(std::env::var("MOCIKA_DEX_RESEARCH_INPUT_DIR").unwrap());
        let report = super::benchmark::run(&input_dir).unwrap();
        println!("{report}");
    }

    #[test]
    #[ignore = "通过环境变量显式评估逐 DEX 独立封装资源成本"]
    fn 评估逐_dex_独立封装资源成本() {
        let input_dir =
            std::path::PathBuf::from(std::env::var("MOCIKA_DEX_RESEARCH_INPUT_DIR").unwrap());
        let report = super::benchmark::run_per_dex(&input_dir).unwrap();
        println!("{report}");
    }

    #[test]
    #[ignore = "通过环境变量显式验证真实多 DEX 分层认证索引"]
    fn 验证真实多_dex_分层认证索引() {
        let input_dir =
            std::path::PathBuf::from(std::env::var("MOCIKA_DEX_RESEARCH_INPUT_DIR").unwrap());
        let report = super::benchmark::verify_layered(&input_dir).unwrap();
        println!("{report}");
    }

    fn dex_sequence(path: &std::path::Path) -> Option<u32> {
        let name = path.file_name()?.to_str()?;
        if name == "classes.dex" {
            return Some(1);
        }
        name.strip_prefix("classes")?
            .strip_suffix(".dex")?
            .parse()
            .ok()
    }

    fn make_test_dex() -> Vec<u8> {
        let strings = ["Lexample/Test;", "V", "I", "<init>", "answer", "nativeCall"];
        let string_ids_offset = HEADER_SIZE;
        let type_ids_offset = string_ids_offset + strings.len() * 4;
        let proto_ids_offset = type_ids_offset + 3 * 4;
        let method_ids_offset = proto_ids_offset + 2 * 12;
        let class_defs_offset = method_ids_offset + 3 * 8;
        let data_offset = class_defs_offset + 32;
        let mut dex = vec![0u8; data_offset];

        let mut string_offsets = Vec::new();
        for value in strings {
            string_offsets.push(dex.len() as u32);
            push_uleb(&mut dex, value.encode_utf16().count() as u32);
            dex.extend_from_slice(value.as_bytes());
            dex.push(0);
        }
        align_four(&mut dex);

        align_four(&mut dex);
        let constructor_code_offset = dex.len() as u32;
        push_code_item(&mut dex, 1, &[0x000e]);
        align_four(&mut dex);
        let answer_code_offset = dex.len() as u32;
        push_code_item(&mut dex, 1, &[0x1012, 0x000f]);
        let class_data_offset = dex.len() as u32;
        dex.extend_from_slice(&[0, 0, 2, 1]);
        push_uleb(&mut dex, 0);
        push_uleb(&mut dex, 0x1_0001);
        push_uleb(&mut dex, constructor_code_offset);
        push_uleb(&mut dex, 2);
        push_uleb(&mut dex, 0x101);
        push_uleb(&mut dex, 0);
        push_uleb(&mut dex, 1);
        push_uleb(&mut dex, 0x1);
        push_uleb(&mut dex, answer_code_offset);

        dex[0..8].copy_from_slice(b"dex\n035\0");
        let file_size = dex.len() as u32;
        let data_size = (dex.len() - data_offset) as u32;
        write_u32(&mut dex, 32, file_size);
        write_u32(&mut dex, 36, HEADER_SIZE as u32);
        write_u32(&mut dex, 40, 0x1234_5678);
        write_u32(&mut dex, 52, NO_INDEX);
        write_u32(&mut dex, 56, strings.len() as u32);
        write_u32(&mut dex, 60, string_ids_offset as u32);
        write_u32(&mut dex, 64, 3);
        write_u32(&mut dex, 68, type_ids_offset as u32);
        write_u32(&mut dex, 72, 2);
        write_u32(&mut dex, 76, proto_ids_offset as u32);
        write_u32(&mut dex, 88, 3);
        write_u32(&mut dex, 92, method_ids_offset as u32);
        write_u32(&mut dex, 96, 1);
        write_u32(&mut dex, 100, class_defs_offset as u32);
        write_u32(&mut dex, 104, data_size);
        write_u32(&mut dex, 108, data_offset as u32);

        for (index, string_offset) in string_offsets.into_iter().enumerate() {
            write_u32(&mut dex, string_ids_offset + index * 4, string_offset);
        }
        write_u32(&mut dex, type_ids_offset, 0);
        write_u32(&mut dex, type_ids_offset + 4, 1);
        write_u32(&mut dex, type_ids_offset + 8, 2);
        write_proto(&mut dex, proto_ids_offset, 1, 1);
        write_proto(&mut dex, proto_ids_offset + 12, 2, 2);
        write_method(&mut dex, method_ids_offset, 0, 0, 3);
        write_method(&mut dex, method_ids_offset + 8, 0, 1, 4);
        write_method(&mut dex, method_ids_offset + 16, 0, 0, 5);
        write_u32(&mut dex, class_defs_offset, 0);
        write_u32(&mut dex, class_defs_offset + 4, 1);
        write_u32(&mut dex, class_defs_offset + 8, NO_INDEX);
        write_u32(&mut dex, class_defs_offset + 12, 0);
        write_u32(&mut dex, class_defs_offset + 16, NO_INDEX);
        write_u32(&mut dex, class_defs_offset + 20, 0);
        write_u32(&mut dex, class_defs_offset + 24, class_data_offset);
        write_u32(&mut dex, class_defs_offset + 28, 0);
        dex
    }

    fn push_code_item(output: &mut Vec<u8>, registers: u16, instructions: &[u16]) {
        align_four(output);
        output.extend_from_slice(&registers.to_le_bytes());
        output.extend_from_slice(&0u16.to_le_bytes());
        output.extend_from_slice(&0u16.to_le_bytes());
        output.extend_from_slice(&0u16.to_le_bytes());
        output.extend_from_slice(&0u32.to_le_bytes());
        output.extend_from_slice(&(instructions.len() as u32).to_le_bytes());
        for instruction in instructions {
            output.extend_from_slice(&instruction.to_le_bytes());
        }
    }

    fn write_proto(output: &mut [u8], offset: usize, shorty: u32, return_type: u32) {
        write_u32(output, offset, shorty);
        write_u32(output, offset + 4, return_type);
        write_u32(output, offset + 8, 0);
    }

    fn write_method(
        output: &mut [u8],
        offset: usize,
        class_index: u16,
        proto_index: u16,
        name_index: u32,
    ) {
        output[offset..offset + 2].copy_from_slice(&class_index.to_le_bytes());
        output[offset + 2..offset + 4].copy_from_slice(&proto_index.to_le_bytes());
        write_u32(output, offset + 4, name_index);
    }

    fn write_u32(output: &mut [u8], offset: usize, value: u32) {
        output[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }

    fn push_uleb(output: &mut Vec<u8>, mut value: u32) {
        loop {
            let mut byte = (value & 0x7f) as u8;
            value >>= 7;
            if value != 0 {
                byte |= 0x80;
            }
            output.push(byte);
            if value == 0 {
                return;
            }
        }
    }

    fn align_four(output: &mut Vec<u8>) {
        while !output.len().is_multiple_of(4) {
            output.push(0);
        }
    }
}

//! 仅用于研究测试的 DEX 方法指令抽取、占位与离线恢复。

use super::{inventory_methods, parse_header, MethodDisposition};
use sha1::Sha1;
use sha2::{Digest, Sha256};
use thiserror::Error;

const DEX_SIGNATURE_OFFSET: usize = 12;
const DEX_SIGNATURE_END: usize = 32;
const DEX_CHECKSUM_OFFSET: usize = 8;
const DEX_CHECKSUM_END: usize = 12;

#[derive(Debug, Error, PartialEq, Eq)]
pub(super) enum TransformError {
    #[error("DEX 解析失败：{0}")]
    Inventory(#[from] super::DexInventoryError),
    #[error("离线转换只接受无尾部附加数据的完整 DEX：file_size={declared}, actual={actual}")]
    TrailingData { declared: usize, actual: usize },
    #[error("方法 {method_index} 缺少可抽取的指令信息")]
    MissingInstructions { method_index: u32 },
    #[error("方法 {method_index} 的返回类型无法生成占位实现：{prototype}")]
    UnsupportedPrototype {
        method_index: u32,
        prototype: String,
    },
    #[error(
        "方法 {method_index} 的指令区越界：offset={offset}, size={size}, file_size={file_size}"
    )]
    InstructionOutOfBounds {
        method_index: u32,
        offset: usize,
        size: usize,
        file_size: usize,
    },
    #[error("代码载荷记录的方法区段发生重叠")]
    OverlappingMethods,
    #[error("占位载体大小与代码载荷不匹配：expected={expected}, actual={actual}")]
    CarrierSizeMismatch { expected: usize, actual: usize },
    #[error("占位载体摘要与代码载荷不匹配")]
    CarrierIdentityMismatch,
    #[error("方法 {method_index} 的占位内容已被修改")]
    StubMismatch { method_index: u32 },
    #[error("DEX 文件过短，无法修复 Header：{actual} 字节")]
    HeaderTooShort { actual: usize },
    #[error("DEX 文件超过 4 GiB，无法写入 Header：{actual} 字节")]
    FileTooLarge { actual: usize },
}

type Result<T> = std::result::Result<T, TransformError>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Extraction {
    pub(super) carrier: Vec<u8>,
    pub(super) payload: CodePayload,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct CodePayload {
    pub(super) carrier_sha256: [u8; 32],
    pub(super) original_file_size: usize,
    pub(super) methods: Vec<ExtractedMethod>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ExtractedMethod {
    pub(super) method_index: u32,
    pub(super) instructions_offset: u32,
    pub(super) original: Vec<u8>,
    pub(super) stub: Vec<u8>,
}

pub(super) fn extract_and_stub(data: &[u8]) -> Result<Extraction> {
    extract_and_stub_where(data, |_| true)
}

pub(super) fn extract_and_stub_selected(
    data: &[u8],
    class_descriptors: &[&str],
) -> Result<Extraction> {
    extract_and_stub_where(data, |class_descriptor| {
        class_descriptors.contains(&class_descriptor)
    })
}

fn extract_and_stub_where(
    data: &[u8],
    mut select_class: impl FnMut(&str) -> bool,
) -> Result<Extraction> {
    let (header, _) = parse_header(data)?;
    if header.file_size != data.len() {
        return Err(TransformError::TrailingData {
            declared: header.file_size,
            actual: data.len(),
        });
    }

    let inventory = inventory_methods(data)?;
    let mut carrier = data.to_vec();
    let mut methods = Vec::new();

    for method in inventory.methods.iter().filter(|method| {
        method.disposition == MethodDisposition::Eligible && select_class(&method.class_descriptor)
    }) {
        let code = method
            .code
            .as_ref()
            .ok_or(TransformError::MissingInstructions {
                method_index: method.method_index,
            })?;
        let offset = code.instructions_offset as usize;
        let size = (code.instructions_size as usize).checked_mul(2).ok_or(
            TransformError::InstructionOutOfBounds {
                method_index: method.method_index,
                offset,
                size: usize::MAX,
                file_size: carrier.len(),
            },
        )?;
        let end = checked_instruction_end(method.method_index, offset, size, carrier.len())?;
        let original = carrier[offset..end].to_vec();
        let stub = make_stub(method.method_index, &method.prototype, size)?;
        carrier[offset..end].copy_from_slice(&stub);
        methods.push(ExtractedMethod {
            method_index: method.method_index,
            instructions_offset: code.instructions_offset,
            original,
            stub,
        });
    }

    methods.sort_by_key(|method| method.instructions_offset);
    validate_non_overlapping(&methods)?;
    repair_header(&mut carrier)?;
    let carrier_sha256 = Sha256::digest(&carrier).into();

    Ok(Extraction {
        payload: CodePayload {
            carrier_sha256,
            original_file_size: data.len(),
            methods,
        },
        carrier,
    })
}

pub(super) fn restore(carrier: &[u8], payload: &CodePayload) -> Result<Vec<u8>> {
    if carrier.len() != payload.original_file_size {
        return Err(TransformError::CarrierSizeMismatch {
            expected: payload.original_file_size,
            actual: carrier.len(),
        });
    }
    if Sha256::digest(carrier).as_slice() != payload.carrier_sha256 {
        return Err(TransformError::CarrierIdentityMismatch);
    }
    validate_non_overlapping(&payload.methods)?;

    let mut restored = carrier.to_vec();
    for method in &payload.methods {
        if method.original.len() != method.stub.len() {
            return Err(TransformError::InstructionOutOfBounds {
                method_index: method.method_index,
                offset: method.instructions_offset as usize,
                size: method.original.len(),
                file_size: restored.len(),
            });
        }
        let offset = method.instructions_offset as usize;
        let end = checked_instruction_end(
            method.method_index,
            offset,
            method.original.len(),
            restored.len(),
        )?;
        if restored[offset..end] != method.stub {
            return Err(TransformError::StubMismatch {
                method_index: method.method_index,
            });
        }
        restored[offset..end].copy_from_slice(&method.original);
    }
    repair_header(&mut restored)?;
    Ok(restored)
}

pub(super) fn bind_stubs_from_carrier(carrier: &[u8], payload: &mut CodePayload) -> Result<()> {
    if carrier.len() != payload.original_file_size {
        return Err(TransformError::CarrierSizeMismatch {
            expected: payload.original_file_size,
            actual: carrier.len(),
        });
    }
    if Sha256::digest(carrier).as_slice() != payload.carrier_sha256 {
        return Err(TransformError::CarrierIdentityMismatch);
    }
    for method in &mut payload.methods {
        let offset = method.instructions_offset as usize;
        let end = checked_instruction_end(
            method.method_index,
            offset,
            method.original.len(),
            carrier.len(),
        )?;
        method.stub = carrier[offset..end].to_vec();
    }
    Ok(())
}

pub(super) fn make_stub(method_index: u32, prototype: &str, capacity: usize) -> Result<Vec<u8>> {
    let return_descriptor = prototype
        .split_once(')')
        .map(|(_, descriptor)| descriptor)
        .filter(|descriptor| !descriptor.is_empty())
        .ok_or_else(|| TransformError::UnsupportedPrototype {
            method_index,
            prototype: prototype.to_string(),
        })?;

    let instructions: &[u16] = match return_descriptor.as_bytes()[0] {
        b'V' => &[0x000e],
        b'J' | b'D' => &[0x0016, 0x0000, 0x0010],
        b'L' | b'[' => &[0x0012, 0x0011],
        b'Z' | b'B' | b'S' | b'C' | b'I' | b'F' => &[0x0012, 0x000f],
        _ => {
            return Err(TransformError::UnsupportedPrototype {
                method_index,
                prototype: prototype.to_string(),
            });
        }
    };
    let required = instructions.len() * 2;
    if required > capacity {
        return Err(TransformError::InstructionOutOfBounds {
            method_index,
            offset: 0,
            size: required,
            file_size: capacity,
        });
    }

    let mut stub = vec![0u8; capacity];
    for (index, instruction) in instructions.iter().enumerate() {
        let offset = index * 2;
        stub[offset..offset + 2].copy_from_slice(&instruction.to_le_bytes());
    }
    Ok(stub)
}

fn checked_instruction_end(
    method_index: u32,
    offset: usize,
    size: usize,
    file_size: usize,
) -> Result<usize> {
    let end = offset
        .checked_add(size)
        .ok_or(TransformError::InstructionOutOfBounds {
            method_index,
            offset,
            size,
            file_size,
        })?;
    if end > file_size {
        return Err(TransformError::InstructionOutOfBounds {
            method_index,
            offset,
            size,
            file_size,
        });
    }
    Ok(end)
}

fn validate_non_overlapping(methods: &[ExtractedMethod]) -> Result<()> {
    let mut previous_end = 0usize;
    for method in methods {
        let start = method.instructions_offset as usize;
        let end = start
            .checked_add(method.original.len())
            .ok_or(TransformError::OverlappingMethods)?;
        if start < previous_end {
            return Err(TransformError::OverlappingMethods);
        }
        previous_end = end;
    }
    Ok(())
}

pub(super) fn repair_header(data: &mut [u8]) -> Result<()> {
    if data.len() < super::HEADER_SIZE {
        return Err(TransformError::HeaderTooShort { actual: data.len() });
    }
    let file_size = u32::try_from(data.len())
        .map_err(|_| TransformError::FileTooLarge { actual: data.len() })?;
    data[32..36].copy_from_slice(&file_size.to_le_bytes());
    let signature = Sha1::digest(&data[DEX_SIGNATURE_END..]);
    data[DEX_SIGNATURE_OFFSET..DEX_SIGNATURE_END].copy_from_slice(&signature);
    let checksum = adler32(&data[DEX_CHECKSUM_END..]);
    data[DEX_CHECKSUM_OFFSET..DEX_CHECKSUM_END].copy_from_slice(&checksum.to_le_bytes());
    Ok(())
}

fn adler32(data: &[u8]) -> u32 {
    const MOD_ADLER: u32 = 65_521;
    let mut a = 1u32;
    let mut b = 0u32;
    for byte in data {
        a = (a + u32::from(*byte)) % MOD_ADLER;
        b = (b + a) % MOD_ADLER;
    }
    (b << 16) | a
}

//! MSHD 尾块定位；兼容原存在性检查，严格读取可继续检查冲突和损坏声明。
use anyhow::{bail, Result};
use std::io::Read;

#[derive(Clone, Copy, Debug)]
pub(crate) struct Block {
    pub offset: u64,
    pub bytes: u32,
}

#[derive(Default)]
pub(crate) struct Scan {
    pub block: Option<Block>,
    pub ambiguous: bool,
    pub damaged: bool,
}

pub(crate) fn scan(
    reader: &mut impl Read,
    size: u64,
    stop_after_match: bool,
    check: &mut impl FnMut() -> Result<()>,
) -> Result<Scan> {
    const CHUNK: usize = 64 * 1024;
    const OVERLAP: usize = 11;
    let mut result = Scan::default();
    let mut buffer = vec![0u8; CHUNK + OVERLAP];
    let mut overlap = 0;
    let mut consumed = 0u64;
    loop {
        check()?;
        let count = reader.read(&mut buffer[overlap..overlap + CHUNK])?;
        if count == 0 {
            if consumed != size {
                bail!("DEX 实际长度与 ZIP 声明不一致");
            }
            return Ok(result);
        }
        let available = overlap + count;
        let base = consumed.saturating_sub(overlap as u64);
        consumed = consumed
            .checked_add(count as u64)
            .ok_or_else(|| anyhow::anyhow!("DEX 长度溢出"))?;
        if consumed > size {
            bail!("DEX 实际长度超过 ZIP 声明");
        }
        for index in 0..available.saturating_sub(7) {
            if &buffer[index..index + 4] != b"MSHD" {
                continue;
            }
            let bytes = u32::from_le_bytes(buffer[index + 4..index + 8].try_into()?);
            let offset = base + index as u64;
            if offset
                .checked_add(8)
                .and_then(|v| v.checked_add(u64::from(bytes)))
                == Some(size)
            {
                match result.block {
                    None => result.block = Some(Block { offset, bytes }),
                    Some(first) if first.offset != offset => result.ambiguous = true,
                    _ => {}
                }
                if stop_after_match {
                    return Ok(result);
                }
            } else if buffer.get(index + 8..index + 12) == Some(b"DEXB") {
                // 普通代码中的魔数文本不作损坏依据；连续的格式声明才保守拒绝。
                result.damaged = true;
            }
        }
        overlap = available.min(OVERLAP);
        buffer.copy_within(available - overlap..available, 0);
    }
}

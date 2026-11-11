//! 有限请求与只读标准输入；不接收密码，不管理子进程或任务终态。
use super::invalid;
use anyhow::Result;
use std::{fs::File, io::Write, path::Path};

pub(super) const MAX: usize = 65536;

pub(super) fn encode(values: &[&str]) -> Result<Vec<u8>> {
    if values.is_empty() || values.len() > 64 {
        return Err(invalid());
    }
    let mut bytes = b"MSJA".to_vec();
    bytes.extend_from_slice(&super::PROTOCOL.to_be_bytes());
    bytes.extend_from_slice(&(values.len() as u32).to_be_bytes());
    for value in values {
        if value.len() > MAX || bytes.len() + 4 + value.len() > MAX {
            return Err(invalid());
        }
        bytes.extend_from_slice(&(value.len() as u32).to_be_bytes());
        bytes.extend_from_slice(value.as_bytes());
    }
    Ok(bytes)
}

pub(super) fn input(directory: &Path, bytes: &[u8]) -> Result<File> {
    if bytes.len() > MAX {
        return Err(invalid());
    }
    let path = directory.join("request.bin");
    super::private_io::validate_directory(directory)?;
    let mut writer = super::private_io::create_file(&path)?;
    writer.write_all(bytes)?;
    writer.flush()?;
    let identity = super::private_io::identity(&writer)?;
    drop(writer);
    // 本次独占私有目录中的新文件；关闭全部写句柄后才交给 Java。
    super::private_io::read_back(&path, identity, bytes.len())
}

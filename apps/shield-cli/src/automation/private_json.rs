//! 私有任务 JSON 的有界序列化与摘要；原生文件身份和权限仍由任务文件模块负责。
use super::{task_files, CheckError};
use serde::{de::DeserializeOwned, Serialize};
use sha2::{Digest, Sha256};
use std::{
    io::{Read, Write},
    path::Path,
};

const MAX_BYTES: u64 = 65536;

fn invalid() -> CheckError {
    CheckError::new(
        "PRIVATE_FILE_INVALID",
        "私有任务文件不可用；不会自动覆盖或重试。",
    )
}

pub(super) fn read<T: DeserializeOwned>(
    path: &Path,
    limit: u64,
) -> Result<(T, Vec<u8>), CheckError> {
    if limit > MAX_BYTES {
        return Err(invalid());
    }
    task_files::validate_name(path.file_name().ok_or_else(invalid)?).map_err(|_| invalid())?;
    let file = task_files::regular(path).map_err(|_| invalid())?;
    task_files::validate_private_file(&file).map_err(|_| invalid())?;
    if file.metadata().map_err(|_| invalid())?.len() > limit || !task_files::same_entry(path, &file)
    {
        return Err(invalid());
    }
    let mut bytes = Vec::new();
    (&file)
        .take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| invalid())?;
    if bytes.len() as u64 > limit || !task_files::same_entry(path, &file) {
        return Err(invalid());
    }
    task_files::validate_private_file(&file).map_err(|_| invalid())?;
    let value = serde_json::from_slice(&bytes).map_err(|_| invalid())?;
    Ok((value, bytes))
}

pub(super) fn write(path: &Path, value: &impl Serialize) -> Result<[u8; 32], CheckError> {
    let bytes = serde_json::to_vec(value).map_err(|_| invalid())?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err(invalid());
    }
    let name = path.file_name().ok_or_else(invalid)?;
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    task_files::validate_name(name).map_err(|_| invalid())?;
    let directory = task_files::inspect_directory(parent).map_err(|_| invalid())?;
    let mut file = task_files::create_at(&directory, parent, name).map_err(|_| invalid())?;
    task_files::validate_private_file(&file).map_err(|_| invalid())?;
    file.write_all(&bytes)
        .and_then(|_| file.sync_all())
        .map_err(|_| invalid())?;
    if !task_files::same_entry(parent, &directory) || !task_files::same_entry(path, &file) {
        return Err(invalid());
    }
    // 出错后不按路径删除或重试；残留由拥有本次目录的任务生命周期处理。
    Ok(Sha256::digest(bytes).into())
}

#[cfg(test)]
mod tests;

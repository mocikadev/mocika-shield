//! 锁内重新读取、按字段差异合并并原子替换 TOML；不缓存整份文档作为写入基线。
use super::{
    locking::ConfigLock, models::BusinessConfig, ConfigError, ProtectDefaults, ProtectDefaultsPatch,
};
use serde::{de::DeserializeOwned, Serialize};
use std::{
    fs::{self, File},
    io::{Read, Write},
    path::PathBuf,
    time::Duration,
};
use toml::Value;

const MAX_BYTES: u64 = 8 * 1024 * 1024;

#[derive(Clone)]
pub struct ConfigStore {
    path: PathBuf,
    lock_timeout: Duration,
    must_exist: bool,
}
impl ConfigStore {
    pub fn new(path: PathBuf) -> Self {
        Self {
            path,
            lock_timeout: Duration::from_secs(3),
            must_exist: false,
        }
    }
    /// 已加载的应用状态不能在文件被删除后用内存快照悄悄重建。
    pub fn existing(path: PathBuf) -> Self {
        Self {
            must_exist: true,
            ..Self::new(path)
        }
    }

    /// 原子替换保证单次读取是某一完整版本；只读不创建目录、锁文件或配置。
    pub fn read<T: DeserializeOwned + Clone>(&self, initial: &T) -> Result<T, ConfigError> {
        match self.document()? {
            Some(document) => document
                .try_into()
                .map_err(|_| ConfigError::InvalidDocument),
            None if !self.must_exist => Ok(initial.clone()),
            None => Err(ConfigError::ReadFailed),
        }
    }

    /// 仅初始化缺失字段；已经存在的未知字段／分区保留，不用启动快照覆盖磁盘。
    pub fn initialize<T>(
        &self,
        initial: &T,
        normalize: impl FnOnce(&mut T) -> Result<(), ConfigError>,
    ) -> Result<T, ConfigError>
    where
        T: DeserializeOwned + Serialize + Clone,
    {
        self.edit(initial, normalize, true)
    }

    /// 回调在有限文件锁内执行，只允许短时间本地状态变更，不得在其中运行 Java 或联网。
    pub fn update<T>(
        &self,
        initial: &T,
        change: impl FnOnce(&mut T) -> Result<(), ConfigError>,
    ) -> Result<T, ConfigError>
    where
        T: DeserializeOwned + Serialize + Clone,
    {
        self.edit(initial, change, false)
    }

    pub fn read_protect_defaults(&self) -> Result<ProtectDefaults, ConfigError> {
        let value: BusinessConfig = self.read(&BusinessConfig::default())?;
        Ok(super::normalize_protect_defaults(&value.protect_defaults))
    }
    pub fn update_protect_defaults(
        &self,
        patch: ProtectDefaultsPatch,
    ) -> Result<ProtectDefaults, ConfigError> {
        let value = self.update(&BusinessConfig::default(), |value| {
            patch.apply(&mut value.protect_defaults)
        })?;
        Ok(value.protect_defaults)
    }

    fn edit<T>(
        &self,
        initial: &T,
        change: impl FnOnce(&mut T) -> Result<(), ConfigError>,
        initialize: bool,
    ) -> Result<T, ConfigError>
    where
        T: DeserializeOwned + Serialize + Clone,
    {
        self.check_path()?;
        let parent = self.path.parent().ok_or(ConfigError::InvalidPath)?;
        fs::create_dir_all(parent).map_err(|_| ConfigError::WriteFailed)?;
        let name = self
            .path
            .file_name()
            .ok_or(ConfigError::InvalidPath)?
            .to_string_lossy();
        let lock_path = parent.join(format!("{name}.lock"));
        let _guard = ConfigLock::acquire(&lock_path, self.lock_timeout)?;
        let existing = self.document()?;
        if existing.is_none() && self.must_exist {
            return Err(ConfigError::ReadFailed);
        }
        let mut current = match &existing {
            Some(document) => document
                .clone()
                .try_into()
                .map_err(|_| ConfigError::InvalidDocument)?,
            None => initial.clone(),
        };
        let before = Value::try_from(&current).map_err(|_| ConfigError::InvalidDocument)?;
        change(&mut current)?;
        let after = Value::try_from(&current).map_err(|_| ConfigError::InvalidDocument)?;
        let missing = existing.is_none();
        let mut document = existing.clone().unwrap_or_else(|| before.clone());
        delta(&mut document, &before, &after);
        if initialize {
            fill_missing(&mut document, &after);
        }
        // 即使无字段差异也先验证文档；缺文件时初始化本次种子，不能创建空 TOML。
        if missing || existing.as_ref() != Some(&document) {
            self.write(&document)?;
        }
        Ok(current)
    }

    fn check_path(&self) -> Result<(), ConfigError> {
        if !self.path.is_absolute()
            || self.path.file_name().is_none()
            || self.path.parent().is_none()
        {
            return Err(ConfigError::InvalidPath);
        }
        match fs::symlink_metadata(&self.path) {
            Ok(meta) if !meta.is_file() => Err(ConfigError::InvalidPath),
            Ok(_) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(_) => Err(ConfigError::ReadFailed),
        }
    }
    fn document(&self) -> Result<Option<Value>, ConfigError> {
        self.check_path()?;
        let file = match File::open(&self.path) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(_) => return Err(ConfigError::ReadFailed),
        };
        let mut bytes = Vec::new();
        file.take(MAX_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| ConfigError::ReadFailed)?;
        if bytes.len() as u64 > MAX_BYTES {
            return Err(ConfigError::TooLarge);
        }
        let text = std::str::from_utf8(&bytes).map_err(|_| ConfigError::InvalidDocument)?;
        let document: Value = toml::from_str(text).map_err(|_| ConfigError::InvalidDocument)?;
        let table = document.as_table().ok_or(ConfigError::InvalidDocument)?;
        if let Some(version) = table.get("schema_version") {
            if version
                .as_integer()
                .is_none_or(|version| !(0..=1).contains(&version))
            {
                return Err(ConfigError::UnsupportedVersion);
            }
        }
        Ok(Some(document))
    }
    fn write(&self, document: &Value) -> Result<(), ConfigError> {
        let body = toml::to_string_pretty(document).map_err(|_| ConfigError::InvalidDocument)?;
        if body.len() as u64 > MAX_BYTES {
            return Err(ConfigError::TooLarge);
        }
        let parent = self.path.parent().ok_or(ConfigError::InvalidPath)?;
        let mut staged = tempfile::Builder::new()
            .prefix(".mocika-config-")
            .tempfile_in(parent)
            .map_err(|_| ConfigError::WriteFailed)?;
        staged
            .write_all(body.as_bytes())
            .map_err(|_| ConfigError::WriteFailed)?;
        staged
            .as_file()
            .sync_all()
            .map_err(|_| ConfigError::WriteFailed)?;
        self.check_path()?;
        staged
            .persist(&self.path)
            .map_err(|_| ConfigError::WriteFailed)?;
        // 提交后不再执行可失败的工作，不将已经替换的配置谎报为未写入。
        Ok(())
    }
}

fn delta(document: &mut Value, before: &Value, after: &Value) {
    if before == after {
        return;
    }
    if let (Some(existing), Some(old), Some(new)) =
        (document.as_table_mut(), before.as_table(), after.as_table())
    {
        for key in old.keys() {
            if !new.contains_key(key) {
                existing.remove(key);
            }
        }
        for (key, value) in new {
            match old.get(key) {
                Some(previous) if previous == value => {}
                Some(previous) => match existing.get_mut(key) {
                    Some(target) => delta(target, previous, value),
                    None => {
                        existing.insert(key.clone(), value.clone());
                    }
                },
                None => {
                    existing.insert(key.clone(), value.clone());
                }
            }
        }
    } else {
        *document = after.clone();
    }
}
fn fill_missing(document: &mut Value, defaults: &Value) {
    if let (Some(existing), Some(defaults)) = (document.as_table_mut(), defaults.as_table()) {
        for (key, value) in defaults {
            match existing.get_mut(key) {
                Some(target) => fill_missing(target, value),
                None => {
                    existing.insert(key.clone(), value.clone());
                }
            }
        }
    }
}

#[cfg(test)]
#[path = "document_tests.rs"]
mod tests;

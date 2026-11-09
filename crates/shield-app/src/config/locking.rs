//! 锁独立的稳定文件；目标 TOML 原子替换时不更换锁身份，不删除锁文件。
use super::ConfigError;
use std::{
    fs::{self, File, OpenOptions, TryLockError},
    path::Path,
    time::{Duration, Instant},
};

pub(super) struct ConfigLock {
    file: File,
}
impl ConfigLock {
    pub fn acquire(path: &Path, timeout: Duration) -> Result<Self, ConfigError> {
        if let Ok(meta) = fs::symlink_metadata(path) {
            if !meta.is_file() {
                return Err(ConfigError::InvalidPath);
            }
        }
        let mut options = OpenOptions::new();
        options.read(true).write(true).create(true).truncate(false);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let file = options.open(path).map_err(|_| ConfigError::LockFailed)?;
        let deadline = Instant::now() + timeout;
        loop {
            match file.try_lock() {
                Ok(()) => return Ok(Self { file }),
                Err(TryLockError::WouldBlock) if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(10))
                }
                Err(TryLockError::WouldBlock) => return Err(ConfigError::LockBusy),
                Err(_) => return Err(ConfigError::LockFailed),
            }
        }
    }
}
impl Drop for ConfigLock {
    fn drop(&mut self) {
        let _ = self.file.unlock();
    }
}

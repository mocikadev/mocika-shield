//! Unix 单 APK 暂存与无覆盖发布；不负责启动进程或证明 APK 合格。
//! 调用发布前必须确认整组工作进程已停止，并完成签名、证书、加固标记及对齐验收。
use super::snapshot;
use rustix::fs::{linkat, openat, AtFlags, Mode, OFlags};
use sha2::{Digest, Sha256};
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use tempfile::{NamedTempFile, TempDir};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum PublicationCheckpoint {
    Copy,
    BeforeCommit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum PublicationError {
    OutputInvalid,
    OutputExists,
    OutputChanged,
    InvalidArtifact,
    DigestMismatch,
    CommitUnconfirmed,
    Cancelled,
    TimedOut,
    Io(io::ErrorKind),
}

impl From<io::Error> for PublicationError {
    fn from(error: io::Error) -> Self {
        // 不把包含真实路径的原始文件系统错误穿透给未来的 Agent 协议。
        Self::Io(error.kind())
    }
}

/// 此类型只记录已发生的提交，不负责把位置不确定映射成对外成功。
pub(super) struct PublicationReceipt {
    pub bytes: u64,
    pub sha256: [u8; 32],
    pub location_confirmed: bool,
    pub directory_synced: bool,
    pub cleanup_complete: bool,
}

pub(super) struct Publication {
    lookup: PathBuf,
    target: PathBuf,
    parent: File,
    directory: File,
    work: Option<TempDir>,
    cleanup_result: Option<bool>,
    cleanup_protected: bool,
}

impl Publication {
    pub fn new(output: &Path) -> Result<Self, PublicationError> {
        if !output
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("apk"))
        {
            return Err(PublicationError::OutputInvalid);
        }
        let lookup = if output.is_absolute() {
            output.to_owned()
        } else {
            std::env::current_dir()?.join(output)
        };
        let target = inspect_output(&lookup)?;
        let parent_path = target.parent().ok_or(PublicationError::OutputChanged)?;
        let parent = File::open(parent_path)?;
        let work = tempfile::Builder::new()
            .prefix(".mocika-task-")
            .permissions(fs::Permissions::from_mode(0o700))
            .tempdir_in(parent_path)?;
        let directory = File::from(
            openat(
                &parent,
                work.path()
                    .file_name()
                    .ok_or(PublicationError::OutputChanged)?,
                OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                Mode::empty(),
            )
            .map_err(io::Error::from)?,
        );
        let publication = Self {
            lookup,
            target,
            parent,
            directory,
            work: Some(work),
            cleanup_result: None,
            cleanup_protected: false,
        };
        publication.recheck()?;
        Ok(publication)
    }

    /// 任务描述只提供此目录作为写目标；这不是操作系统权限沙箱。
    pub fn work_dir(&self) -> Result<&Path, PublicationError> {
        self.work
            .as_ref()
            .map(TempDir::path)
            .ok_or(PublicationError::InvalidArtifact)
    }

    /// 仅发布固定的 signed.apk，不接受任意源路径或 V4 多产物。
    /// checkpoint 由父进程检查总期限与取消；文件系统调用本身不是硬实时可中断的。
    pub fn publish_after_stop(
        &mut self,
        expected_sha256: [u8; 32],
        mut checkpoint: impl FnMut(PublicationCheckpoint) -> Result<(), PublicationError>,
    ) -> Result<PublicationReceipt, PublicationError> {
        if self.cleanup_protected {
            return Err(PublicationError::CommitUnconfirmed);
        }
        self.recheck()?;
        checkpoint(PublicationCheckpoint::Copy)?;
        let (mut sealed, bytes, sha256) = self.seal(expected_sha256, &mut checkpoint)?;
        self.recheck()?;
        if !same_directory_entry(sealed.path(), sealed.as_file()) {
            return Err(PublicationError::InvalidArtifact);
        }
        let source_name = sealed
            .path()
            .file_name()
            .ok_or(PublicationError::InvalidArtifact)?;
        let target_name = self
            .target
            .file_name()
            .ok_or(PublicationError::OutputChanged)?;
        // 这是最后一个可取消点。调用方据此进入提交临界区，直到收到确定回执才定终态。
        checkpoint(PublicationCheckpoint::BeforeCommit)?;
        self.cleanup_protected = true;
        // 相对已固定的目录句柄链接，原子创建最终名字；绝不回退到覆盖或直接复制。
        let linked = linkat(
            &self.directory,
            source_name,
            &self.parent,
            target_name,
            AtFlags::empty(),
        );
        if let Err(error) = linked {
            if io::Error::from(error).kind() == io::ErrorKind::AlreadyExists {
                self.cleanup_protected = false;
                return Err(PublicationError::OutputExists);
            }
            // 网络文件系统等可能在提交后丢失回执；其他链接错误保守视为未知。
            // 保留本次材料，不把它映射成确定无输出，也不自动重试。
            sealed.disable_cleanup(true);
            return Err(PublicationError::CommitUnconfirmed);
        }
        self.cleanup_protected = false;

        // 从此处起已经提交，后续失败只能成为回执中的附加状态，不能返回无输出错误。
        let location_confirmed =
            self.parent_matches() && same_directory_entry(&self.target, sealed.as_file());
        let directory_synced = self.parent.sync_all().is_ok();
        drop(sealed);
        let cleanup_complete = self.cleanup();
        Ok(PublicationReceipt {
            bytes,
            sha256,
            location_confirmed,
            directory_synced,
            cleanup_complete,
        })
    }

    fn seal(
        &self,
        expected: [u8; 32],
        checkpoint: &mut impl FnMut(PublicationCheckpoint) -> Result<(), PublicationError>,
    ) -> Result<(NamedTempFile, u64, [u8; 32]), PublicationError> {
        // NONBLOCK 防止特殊文件在确认类型前阻塞；NOFOLLOW 拒绝链接产物。
        let mut artifact = File::from(
            openat(
                &self.directory,
                "signed.apk",
                OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
                Mode::empty(),
            )
            .map_err(|_| PublicationError::InvalidArtifact)?,
        );
        let metadata = artifact.metadata()?;
        if !metadata.is_file() || metadata.len() == 0 || metadata.nlink() != 1 {
            return Err(PublicationError::InvalidArtifact);
        }
        // 用父进程新建的独立 inode 固化最终字节，避免沿用工作进程曾持有的写句柄。
        let mut sealed =
            tempfile::Builder::new()
                .prefix(".ready-")
                .make_in(self.work_dir()?, |path| {
                    let name = path.file_name().ok_or(io::ErrorKind::InvalidInput)?;
                    openat(
                        &self.directory,
                        name,
                        OFlags::RDWR | OFlags::CREATE | OFlags::EXCL | OFlags::CLOEXEC,
                        Mode::RUSR | Mode::WUSR,
                    )
                    .map(File::from)
                    .map_err(io::Error::from)
                })?;
        let mut hash = Sha256::new();
        let mut bytes = 0;
        let mut buffer = [0u8; 64 * 1024];
        loop {
            checkpoint(PublicationCheckpoint::Copy)?;
            let count = artifact.read(&mut buffer)?;
            if count == 0 {
                break;
            }
            sealed.write_all(&buffer[..count])?;
            hash.update(&buffer[..count]);
            bytes += count as u64;
        }
        let sha256: [u8; 32] = hash.finalize().into();
        if bytes != metadata.len() || sha256 != expected {
            return Err(PublicationError::DigestMismatch);
        }
        sealed.as_file().sync_all()?;
        Ok((sealed, bytes, sha256))
    }

    fn recheck(&self) -> Result<(), PublicationError> {
        if !self.parent_matches()
            || !same_directory_entry(self.work_dir()?, &self.directory)
            || inspect_output(&self.lookup)? != self.target
        {
            return Err(PublicationError::OutputChanged);
        }
        Ok(())
    }

    fn parent_matches(&self) -> bool {
        let Some(parent) = self.lookup.parent() else {
            return false;
        };
        let Ok(resolved) = fs::canonicalize(parent) else {
            return false;
        };
        self.target.parent() == Some(resolved.as_path())
            && same_directory_entry(&resolved, &self.parent)
    }

    /// 失败路径须在发终态前显式调用并记录结果；Drop 仅作最后兜底。
    pub fn preserve(&mut self) {
        if let Some(work) = self.work.take() {
            let _ = work.keep();
        }
        self.cleanup_result = Some(false);
        self.cleanup_protected = true;
    }

    pub fn cleanup(&mut self) -> bool {
        let Some(work) = self.work.take() else {
            return self.cleanup_result.unwrap_or(true);
        };
        if self.cleanup_protected || !same_directory_entry(work.path(), &self.directory) {
            // 身份不符就不递归清理该路径，避免碰到被替换的目录。
            let _ = work.keep();
            self.cleanup_result = Some(false);
            return false;
        }
        let complete = work.close().is_ok();
        self.cleanup_result = Some(complete);
        complete
    }
}

impl Drop for Publication {
    fn drop(&mut self) {
        let _ = self.cleanup();
    }
}

fn inspect_output(path: &Path) -> Result<PathBuf, PublicationError> {
    snapshot::output(path).map_err(|error| {
        if error.code == "OUTPUT_EXISTS" {
            PublicationError::OutputExists
        } else {
            PublicationError::OutputChanged
        }
    })
}

fn same_directory_entry(path: &Path, file: &File) -> bool {
    let (Ok(current), Ok(expected)) = (fs::symlink_metadata(path), file.metadata()) else {
        return false;
    };
    current.dev() == expected.dev() && current.ino() == expected.ino()
}

#[cfg(test)]
#[path = "publication_tests.rs"]
mod tests;

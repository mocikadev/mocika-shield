//! 普通签名的暂存与失败回滚；不承担 Agent 最终无覆盖发布或进程停止。
use anyhow::{bail, Context, Result};
use std::fs;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

pub(super) struct Artifacts {
    directory: TempDir,
    output: PathBuf,
    v4: bool,
}

fn sidecar(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(".idsig");
    name.into()
}

fn existing_file(path: &Path) -> Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_file() => Ok(true),
        Ok(_) => bail!("签名输出或附件不是普通文件，未替换任何原产物"),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error).context("无法检查签名输出，未替换任何原产物"),
    }
}

fn require_nonempty(path: &Path) -> Result<()> {
    if !existing_file(path)? || fs::metadata(path)?.len() == 0 {
        bail!("签名器未生成完整 APK 或 V4 附件，未替换任何原产物");
    }
    Ok(())
}

impl Artifacts {
    pub(super) fn new(output: &Path, v4: bool) -> Result<Self> {
        existing_file(output)?;
        existing_file(&sidecar(output))?;
        let parent = output
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        let directory = tempfile::Builder::new()
            .prefix(".mocika-sign-")
            .tempdir_in(parent)
            .context("无法在输出目录内创建签名暂存材料，未替换任何原产物")?;
        Ok(Self {
            directory,
            output: output.to_owned(),
            v4,
        })
    }

    pub(super) fn staged_apk(&self) -> PathBuf {
        self.directory.path().join("signed.apk")
    }

    pub(super) fn validate(&self) -> Result<()> {
        require_nonempty(&self.staged_apk())?;
        if self.v4 {
            require_nonempty(&sidecar(&self.staged_apk()))?;
        }
        Ok(())
    }

    pub(super) fn publish(self) -> Result<()> {
        self.publish_with(|from, to| fs::rename(from, to))
    }

    fn publish_with(
        self,
        mut rename: impl FnMut(&Path, &Path) -> std::io::Result<()>,
    ) -> Result<()> {
        self.validate()?;
        let targets = [self.output.clone(), sidecar(&self.output)];
        let staged = [self.staged_apk(), sidecar(&self.staged_apk())];
        let backups = [
            self.directory.path().join("previous.apk"),
            self.directory.path().join("previous.idsig"),
        ];
        let existed = [existing_file(&targets[0])?, existing_file(&targets[1])?];
        let mut saved = [false; 2];
        let mut published = [false; 2];
        let result = (|| -> std::io::Result<()> {
            for i in 0..2 {
                if existed[i] {
                    rename(&targets[i], &backups[i])?;
                    saved[i] = true;
                }
            }
            // 附件先提交，APK 最后提交；这是可回滚的多文件操作，不是原子事务。
            if self.v4 {
                rename(&staged[1], &targets[1])?;
                published[1] = true;
            }
            rename(&staged[0], &targets[0])?;
            published[0] = true;
            Ok(())
        })();
        if let Err(error) = result {
            let mut restored = true;
            for i in (0..2).rev() {
                if published[i] && rename(&targets[i], &staged[i]).is_err() {
                    restored = false;
                }
                if saved[i] && rename(&backups[i], &targets[i]).is_err() {
                    restored = false;
                }
            }
            if !restored {
                let recovery = self.directory.keep();
                bail!(
                    "签名提交失败且旧产物恢复未完成，恢复材料已保留于 {}",
                    recovery.display()
                );
            }
            return Err(error).context("签名提交失败，旧产物已恢复");
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;

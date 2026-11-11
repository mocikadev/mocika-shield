//! 借用工作根的固定身份和 Java 引导落点；不拥有清理权或任务停止证明。
use anyhow::Result;
use std::{
    fs,
    fs::File,
    path::{Path, PathBuf},
};

fn invalid() -> anyhow::Error {
    crate::diagnostic::Diagnostic::new(crate::diagnostic::FailureCode::ToolProcessFailed).attach(
        anyhow::anyhow!("Java 引导目录无法安全表示或身份已变化；未启动工具，不会自动重试。"),
    )
}

pub(super) struct Workspace {
    path: PathBuf,
    pinned: File,
}

impl Workspace {
    pub fn new(path: &Path) -> Result<Self> {
        if !path.is_absolute() || !fs::symlink_metadata(path)?.is_dir() {
            return Err(invalid());
        }
        #[cfg(windows)]
        let pinned = crate::java_directory::open_directory(path)?;
        #[cfg(unix)]
        let pinned = File::open(path)?;
        let workspace = Self {
            path: crate::utils::strip_unc_prefix(&fs::canonicalize(path)?),
            pinned,
        };
        workspace.verify()?;
        Ok(workspace)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn verify(&self) -> Result<()> {
        #[cfg(windows)]
        let matches = crate::java_directory::matches_directory(&self.path, &self.pinned)?;
        #[cfg(windows)]
        crate::windows_private::validate(&self.pinned, true)?;
        #[cfg(unix)]
        let matches = {
            use std::os::unix::fs::MetadataExt;
            let current = fs::symlink_metadata(&self.path)?;
            let expected = self.pinned.metadata()?;
            current.is_dir()
                && current.dev() == expected.dev()
                && current.ino() == expected.ino()
                && current.mode() & 0o077 == 0
                && current.uid() == unsafe { libc::geteuid() }
        };
        if !matches {
            return Err(invalid());
        }
        Ok(())
    }
}

pub(super) fn bootstrap_directory(path: &Path) -> Result<PathBuf> {
    let text = path.to_str().ok_or_else(invalid)?;
    if text.chars().any(|ch| ch as u32 > 0xffff) {
        // 原生证据表明 ASCII 短名及 user.dir 也不能修复这类 JAR 引导，不能启动后重试。
        return Err(crate::diagnostic::Diagnostic::new(
            crate::diagnostic::FailureCode::ToolProcessFailed,
        )
        .attach(anyhow::anyhow!(
            "Java 引导工作目录包含当前不支持的补充字符；未启动工具，不会自动改换目录或重试。"
        )));
    }
    #[cfg(windows)]
    {
        use std::path::{Component, Prefix};
        let extended = matches!(path.components().next(), Some(Component::Prefix(prefix)) if matches!(
            prefix.kind(), Prefix::Verbatim(_) | Prefix::VerbatimUNC(_, _) | Prefix::VerbatimDisk(_) | Prefix::DeviceNS(_)
        ));
        if !extended {
            return Ok(path.to_owned());
        }
        crate::java_directory::ascii_directory(path).map_err(|_| invalid())
    }
    #[cfg(not(windows))]
    {
        Ok(path.to_owned())
    }
}

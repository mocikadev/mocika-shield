//! 宿主固定的共享上下文与证书白名单；不读取数据库、配置内容或秘密。
use super::{Result, Root};
use shield_app::paths::ApplicationPaths;
use std::{
    collections::BTreeSet,
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
};

struct Directory {
    lookup: PathBuf,
    anchor_lookup: PathBuf,
    anchor: Root,
}
impl Directory {
    fn new(path: PathBuf) -> Result<Self> {
        let mut anchor_lookup = path.clone();
        while !anchor_lookup
            .try_exists()
            .map_err(|_| "SHARED_CONTEXT_INVALID")?
        {
            if !anchor_lookup.pop() {
                return Err("SHARED_CONTEXT_INVALID");
            }
        }
        let anchor = Root::open(&anchor_lookup)?;
        let directory = Self {
            lookup: path,
            anchor_lookup,
            anchor,
        };
        directory.check()?;
        Ok(directory)
    }
    fn check(&self) -> Result<()> {
        self.anchor.check()?;
        if fs::canonicalize(&self.anchor_lookup).map_err(|_| "SHARED_CONTEXT_CHANGED")?
            != self.anchor.path
        {
            return Err("SHARED_CONTEXT_CHANGED");
        }
        let tail = self
            .lookup
            .strip_prefix(&self.anchor_lookup)
            .map_err(|_| "SHARED_CONTEXT_INVALID")?;
        let mut current = self.anchor_lookup.clone();
        for part in tail.components() {
            current.push(part);
            match fs::symlink_metadata(&current) {
                Ok(meta) if meta.is_dir() && !meta.file_type().is_symlink() => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
                _ => return Err("SHARED_CONTEXT_CHANGED"),
            }
        }
        Ok(())
    }
    fn overlaps(&self, output: &Path) -> Result<bool> {
        let tail = self
            .lookup
            .strip_prefix(&self.anchor_lookup)
            .map_err(|_| "SHARED_CONTEXT_INVALID")?;
        let physical = self.anchor.path.join(tail);
        Ok(physical.starts_with(output) || output.starts_with(physical))
    }
}

pub(super) struct SharedAccess {
    home: Option<PathBuf>,
    paths: ApplicationPaths,
    config: Directory,
    data: Directory,
    allowed: BTreeSet<String>,
}
impl SharedAccess {
    pub fn new(home: Option<&Path>, ids: Vec<String>, output: &Path) -> Result<Self> {
        if ids.is_empty() || ids.len() > 64 || ids.iter().any(|id| !valid_id(id)) {
            return Err("CERTIFICATE_SCOPE_REQUIRED");
        }
        let count = ids.len();
        let allowed: BTreeSet<_> = ids.into_iter().collect();
        if allowed.len() != count {
            return Err("CERTIFICATE_SCOPE_INVALID");
        }
        let paths = home
            .map_or_else(ApplicationPaths::current_user, ApplicationPaths::isolated)
            .map_err(|_| "SHARED_CONTEXT_INVALID")?;
        let config = Directory::new(
            paths
                .config_file()
                .parent()
                .ok_or("SHARED_CONTEXT_INVALID")?
                .to_owned(),
        )?;
        let data = Directory::new(
            paths
                .certificate_database()
                .parent()
                .ok_or("SHARED_CONTEXT_INVALID")?
                .to_owned(),
        )?;
        if config.overlaps(output)? || data.overlaps(output)? {
            return Err("SHARED_CONTEXT_IN_OUTPUT_ROOT");
        }
        let access = Self {
            home: home.map(Path::to_owned),
            paths,
            config,
            data,
            allowed,
        };
        access.check()?;
        Ok(access)
    }
    pub fn check(&self) -> Result<()> {
        self.config.check()?;
        self.data.check()?;
        // 只查文件形态，不固定整库 inode／摘要；正常 SQLite 更新不使无关计划失效。
        for path in [self.paths.config_file(), self.paths.certificate_database()] {
            match fs::symlink_metadata(path) {
                Ok(meta) if meta.is_file() && !meta.file_type().is_symlink() => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                _ => return Err("SHARED_CONTEXT_CHANGED"),
            }
        }
        Ok(())
    }
    pub fn certificate(&self, id: &str) -> Result<()> {
        if valid_id(id) && self.allowed.contains(id) {
            Ok(())
        } else {
            Err("CERTIFICATE_NOT_ALLOWED")
        }
    }
    pub fn arguments(&self) -> Vec<OsString> {
        self.home
            .as_ref()
            .map(|home| vec!["--app-home".into(), home.as_os_str().to_owned()])
            .unwrap_or_default()
    }
}
fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
}

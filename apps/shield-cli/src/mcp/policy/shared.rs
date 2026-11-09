//! 宿主固定上下文与会话证书范围；只接收已确认的新编号，不读取数据库或秘密。
use super::{Result, Root};
use shield_services::paths::ApplicationPaths;
use std::{
    collections::BTreeSet,
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
    sync::Mutex,
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
    allowed: Mutex<BTreeSet<String>>,
}
impl SharedAccess {
    pub fn new(home: Option<&Path>, ids: Vec<String>, output: &Path) -> Result<Self> {
        if ids.len() > 64 || ids.iter().any(|id| !valid_id(id)) {
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
            allowed: Mutex::new(allowed),
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
        if valid_id(id)
            && self
                .allowed
                .lock()
                .map_err(|_| "CERTIFICATE_SCOPE_INVALID")?
                .contains(id)
        {
            Ok(())
        } else {
            Err("CERTIFICATE_NOT_ALLOWED")
        }
    }
    pub fn sole_certificate(&self) -> Result<Option<String>> {
        let allowed = self
            .allowed
            .lock()
            .map_err(|_| "CERTIFICATE_SCOPE_INVALID")?;
        Ok((allowed.len() == 1).then(|| allowed.first().unwrap().clone()))
    }
    pub fn can_add(&self) -> Result<()> {
        if self
            .allowed
            .lock()
            .map_err(|_| "CERTIFICATE_SCOPE_INVALID")?
            .len()
            >= 64
        {
            Err("CERTIFICATE_SCOPE_FULL")
        } else {
            Ok(())
        }
    }
    pub fn add_created(&self, id: &str) -> Result<()> {
        if !valid_id(id) {
            return Err("CLI_PROTOCOL_INVALID");
        }
        let mut allowed = self
            .allowed
            .lock()
            .map_err(|_| "CERTIFICATE_SCOPE_INVALID")?;
        if allowed.len() >= 64 || allowed.contains(id) {
            return Err("CERTIFICATE_SCOPE_INVALID");
        }
        allowed.insert(id.to_owned());
        Ok(())
    }
    pub fn remove(&self, id: &str) -> Result<()> {
        self.allowed
            .lock()
            .map_err(|_| "CERTIFICATE_SCOPE_INVALID")?
            .remove(id);
        Ok(())
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

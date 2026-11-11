//! 宿主启动时固定的信任范围；不从工具调用接收配置、密码或可执行路径。
use sha2::{Digest, Sha256};
use std::fs::{self, File, Metadata};
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};
mod identity;
mod shared;
use identity::{identity, stamp, Identity};
use shared::SharedAccess;

pub(super) type Result<T> = std::result::Result<T, &'static str>;

fn physical(path: &Path) -> std::io::Result<PathBuf> {
    fs::canonicalize(path).map(|path| shield_core::utils::strip_unc_prefix(&path))
}
fn redirected(meta: &Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        meta.file_attributes() & 0x400 != 0
    }
    #[cfg(unix)]
    meta.file_type().is_symlink()
}
fn directory(path: &Path) -> Result<File> {
    #[cfg(windows)]
    let file = crate::automation::task_files::inspect_directory(path);
    #[cfg(unix)]
    let file = File::open(path);
    file.map_err(|_| "ROOT_INVALID")
}

struct Root {
    path: PathBuf,
    identity: Identity,
}
impl Root {
    fn open(path: &Path) -> Result<Self> {
        if !path.is_absolute() {
            return Err("ROOT_INVALID");
        }
        let path = physical(path).map_err(|_| "ROOT_INVALID")?;
        if path.parent().is_none() {
            return Err("ROOT_INVALID");
        }
        let file = directory(&path)?;
        let meta = file.metadata().map_err(|_| "ROOT_INVALID")?;
        if !meta.is_dir() {
            return Err("ROOT_INVALID");
        }
        Ok(Self {
            path,
            identity: identity(&file, &meta)?,
        })
    }

    fn check(&self) -> Result<()> {
        let meta = fs::symlink_metadata(&self.path).map_err(|_| "ROOT_CHANGED")?;
        let file = directory(&self.path).map_err(|_| "ROOT_CHANGED")?;
        if !meta.is_dir()
            || redirected(&meta)
            || identity(&file, &meta).map_err(|_| "ROOT_CHANGED")? != self.identity
            || physical(&self.path).map_err(|_| "ROOT_CHANGED")? != self.path
        {
            return Err("ROOT_CHANGED");
        }
        Ok(())
    }

    fn resolve(&self, relative: &str, output: bool) -> Result<PathBuf> {
        self.resolve_mode(relative, output, false)
    }

    fn resolve_mode(&self, relative: &str, output: bool, published: bool) -> Result<PathBuf> {
        self.check()?;
        #[cfg(windows)]
        if relative.contains(['\\', ':']) {
            return Err("PATH_NOT_ALLOWED");
        }
        if relative.is_empty()
            || relative.len() > 2048
            || relative.chars().any(char::is_control)
            || relative
                .split('/')
                .any(|v| v.is_empty() || v == "." || v == "..")
        {
            return Err("PATH_NOT_ALLOWED");
        }
        let mut path = self.path.clone();
        let components: Vec<_> = Path::new(relative).components().collect();
        if components.is_empty() {
            return Err("PATH_NOT_ALLOWED");
        }
        for (index, component) in components.iter().enumerate() {
            let Component::Normal(name) = component else {
                return Err("PATH_NOT_ALLOWED");
            };
            #[cfg(windows)]
            crate::automation::task_files::validate_name(name).map_err(|_| "PATH_NOT_ALLOWED")?;
            if output
                && (name
                    .to_string_lossy()
                    .to_ascii_lowercase()
                    .ends_with(".app")
                    || *name == "skills")
            {
                return Err("PATH_NOT_ALLOWED");
            }
            path.push(name);
            let last = index + 1 == components.len();
            match fs::symlink_metadata(&path) {
                Ok(meta) => {
                    if redirected(&meta) {
                        return Err("PATH_NOT_ALLOWED");
                    }
                    if last {
                        if output && !published {
                            return Err("OUTPUT_EXISTS");
                        }
                        if !meta.is_file() {
                            return Err("INPUT_NOT_FILE");
                        }
                    } else if !meta.is_dir() {
                        return Err("PATH_NOT_ALLOWED");
                    }
                }
                Err(error)
                    if last
                        && output
                        && !published
                        && error.kind() == std::io::ErrorKind::NotFound => {}
                Err(_) => return Err("PATH_UNAVAILABLE"),
            }
        }
        Ok(path)
    }
}

struct PinnedFile {
    path: PathBuf,
    physical: PathBuf,
    identity: Identity,
    digest: [u8; 32],
}
impl PinnedFile {
    fn open(path: &Path, limit: u64) -> Result<Self> {
        if !path.is_absolute() {
            return Err("TRUSTED_FILE_INVALID");
        }
        let physical = physical(path).map_err(|_| "TRUSTED_FILE_INVALID")?;
        // 保留原查找位置，使配置中的相对路径语义不因链接发生变化。
        #[cfg(windows)]
        let opened = crate::automation::task_files::regular(&physical);
        #[cfg(unix)]
        let opened = File::open(&physical);
        let mut file = opened.map_err(|_| "TRUSTED_FILE_INVALID")?;
        let meta = file.metadata().map_err(|_| "TRUSTED_FILE_INVALID")?;
        if !meta.is_file() || meta.len() > limit {
            return Err("TRUSTED_FILE_INVALID");
        }
        let before = stamp(&file, &meta)?;
        let mut hash = Sha256::new();
        let count = std::io::copy(&mut (&mut file).take(limit + 1), &mut HashWriter(&mut hash))
            .map_err(|_| "TRUSTED_FILE_INVALID")?;
        let after = file.metadata().map_err(|_| "TRUSTED_FILE_INVALID")?;
        if count != meta.len() || count > limit || before != stamp(&file, &after)? {
            return Err("TRUSTED_FILE_CHANGED");
        }
        if self::physical(path).map_err(|_| "TRUSTED_FILE_CHANGED")? != physical {
            return Err("TRUSTED_FILE_CHANGED");
        }
        #[cfg(windows)]
        if !crate::automation::task_files::same_entry(&physical, &file) {
            return Err("TRUSTED_FILE_CHANGED");
        }
        Ok(Self {
            path: path.to_owned(),
            physical,
            identity: identity(&file, &after)?,
            digest: hash.finalize().into(),
        })
    }

    fn check(&self, limit: u64) -> Result<()> {
        let now = Self::open(&self.path, limit)?;
        if now.physical != self.physical
            || now.identity != self.identity
            || now.digest != self.digest
        {
            return Err("TRUSTED_FILE_CHANGED");
        }
        Ok(())
    }
}

struct HashWriter<'a>(&'a mut Sha256);
impl Write for HashWriter<'_> {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.update(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

pub(super) struct Policy {
    input: Root,
    output: Root,
    cli: PinnedFile,
    config: Option<PinnedFile>,
    shared: Option<SharedAccess>,
    pub timeout_seconds: u64,
}

/// 只在本次导入调用期间固定材料，不将路径或内容交给协议层。
pub(super) struct ImportSource {
    relative: String,
    file: PinnedFile,
}
impl ImportSource {
    pub fn check(&self, policy: &Policy) -> Result<()> {
        if policy.input(&self.relative)? != self.file.path {
            return Err("TRUSTED_FILE_CHANGED");
        }
        self.file.check(16 * 1024 * 1024)
    }
}
impl Policy {
    pub fn new(
        input: &Path,
        output: &Path,
        config: Option<&Path>,
        cli: &Path,
        timeout_seconds: u64,
    ) -> Result<Self> {
        let input = Root::open(input)?;
        let output = Root::open(output)?;
        // 首版拒绝把输出放进任意应用包/技能目录，也拒绝用工具目录作为业务根。
        for path in [&input.path, &output.path] {
            if path.components().any(|component| match component {
                Component::Normal(name) => {
                    name.to_string_lossy()
                        .to_ascii_lowercase()
                        .ends_with(".app")
                        || name == "skills"
                }
                _ => false,
            }) {
                return Err("ROOT_NOT_ALLOWED");
            }
        }
        let cli = PinnedFile::open(cli, 256 * 1024 * 1024)?;
        let config = config
            .map(|path| PinnedFile::open(path, 64 * 1024))
            .transpose()?;
        if config
            .as_ref()
            .is_some_and(|config| config.physical.starts_with(&output.path))
        {
            return Err("CONFIG_IN_OUTPUT_ROOT");
        }
        if !(1..=3600).contains(&timeout_seconds) {
            return Err("TIMEOUT_INVALID");
        }
        Ok(Self {
            input,
            output,
            cli,
            config,
            shared: None,
            timeout_seconds,
        })
    }

    pub fn check(&self) -> Result<()> {
        self.input.check()?;
        self.output.check()?;
        self.cli.check(256 * 1024 * 1024)?;
        if let Some(config) = &self.config {
            config.check(64 * 1024)?;
        }
        if let Some(shared) = &self.shared {
            shared.check()?;
        }
        Ok(())
    }
    pub fn with_shared(mut self, home: Option<&Path>, ids: Vec<String>) -> Result<Self> {
        self.shared = Some(SharedAccess::new(home, ids, &self.output.path)?);
        Ok(self)
    }
    pub fn is_shared(&self) -> bool {
        self.shared.is_some()
    }
    pub fn certificate(&self, id: &str) -> Result<()> {
        self.shared
            .as_ref()
            .ok_or("SHARED_SOURCE_NOT_ENABLED")?
            .certificate(id)
    }
    pub fn shared_arguments(&self) -> Result<Vec<std::ffi::OsString>> {
        Ok(self
            .shared
            .as_ref()
            .ok_or("SHARED_SOURCE_NOT_ENABLED")?
            .arguments())
    }
    pub fn sole_certificate(&self) -> Result<Option<String>> {
        self.shared
            .as_ref()
            .ok_or("SHARED_SOURCE_NOT_ENABLED")?
            .sole_certificate()
    }
    pub fn can_add_certificate(&self) -> Result<()> {
        self.shared
            .as_ref()
            .ok_or("SHARED_SOURCE_NOT_ENABLED")?
            .can_add()
    }
    pub fn add_created_certificate(&self, id: &str) -> Result<()> {
        self.shared
            .as_ref()
            .ok_or("SHARED_SOURCE_NOT_ENABLED")?
            .add_created(id)
    }
    pub fn remove_certificate(&self, id: &str) -> Result<()> {
        self.shared
            .as_ref()
            .ok_or("SHARED_SOURCE_NOT_ENABLED")?
            .remove(id)
    }
    pub fn import_source(&self, relative: &str) -> Result<ImportSource> {
        Ok(ImportSource {
            relative: relative.to_owned(),
            file: PinnedFile::open(&self.input(relative)?, 16 * 1024 * 1024)?,
        })
    }
    pub fn project_config(&self) -> Option<&Path> {
        self.config.as_ref().map(|file| file.path.as_path())
    }
    pub fn input(&self, path: &str) -> Result<PathBuf> {
        self.input.resolve(path, false)
    }
    pub fn output(&self, path: &str) -> Result<PathBuf> {
        self.output.resolve(path, true)
    }
    pub fn published_output(&self, path: &str) -> Result<PathBuf> {
        self.output.resolve_mode(path, true, true)
    }
    pub fn cli(&self) -> &Path {
        &self.cli.path
    }
    pub fn config(&self) -> Result<&Path> {
        self.config
            .as_ref()
            .map(|v| v.path.as_path())
            .ok_or("SIGNING_CONFIG_REQUIRED")
    }
    pub fn working_dir(&self) -> &Path {
        // 业务输入是不可信内容，不能以其所在目录作为工具查找基准。
        self.cli.physical.parent().expect("可信执行器必须有父目录")
    }
}

#[cfg(test)]
mod tests;

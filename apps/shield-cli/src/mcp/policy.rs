//! 宿主启动时固定的信任范围；不从工具调用接收配置、密码或可执行路径。
use sha2::{Digest, Sha256};
use std::fs::{self, File, Metadata};
use std::io::{Read, Write};
use std::os::unix::fs::MetadataExt;
use std::path::{Component, Path, PathBuf};
mod shared;
use shared::SharedAccess;

pub(super) type Result<T> = std::result::Result<T, &'static str>;

#[derive(Clone, PartialEq, Eq)]
struct Identity {
    dev: u64,
    ino: u64,
}
impl Identity {
    fn of(meta: &Metadata) -> Self {
        Self {
            dev: meta.dev(),
            ino: meta.ino(),
        }
    }
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
        let path = fs::canonicalize(path).map_err(|_| "ROOT_INVALID")?;
        if path.parent().is_none() {
            return Err("ROOT_INVALID");
        }
        let meta = fs::metadata(&path).map_err(|_| "ROOT_INVALID")?;
        if !meta.is_dir() {
            return Err("ROOT_INVALID");
        }
        Ok(Self {
            path,
            identity: Identity::of(&meta),
        })
    }

    fn check(&self) -> Result<()> {
        let meta = fs::symlink_metadata(&self.path).map_err(|_| "ROOT_CHANGED")?;
        if !meta.is_dir()
            || Identity::of(&meta) != self.identity
            || fs::canonicalize(&self.path).map_err(|_| "ROOT_CHANGED")? != self.path
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
                    if meta.file_type().is_symlink() {
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
        let physical = fs::canonicalize(path).map_err(|_| "TRUSTED_FILE_INVALID")?;
        // 保留原查找位置，使配置中的相对路径语义不因链接发生变化。
        let mut file = File::open(&physical).map_err(|_| "TRUSTED_FILE_INVALID")?;
        let meta = file.metadata().map_err(|_| "TRUSTED_FILE_INVALID")?;
        if !meta.is_file() || meta.len() > limit {
            return Err("TRUSTED_FILE_INVALID");
        }
        let mut hash = Sha256::new();
        let count = std::io::copy(&mut (&mut file).take(limit + 1), &mut HashWriter(&mut hash))
            .map_err(|_| "TRUSTED_FILE_INVALID")?;
        let after = file.metadata().map_err(|_| "TRUSTED_FILE_INVALID")?;
        if count != meta.len() || count > limit || stamp(&meta) != stamp(&after) {
            return Err("TRUSTED_FILE_CHANGED");
        }
        if fs::canonicalize(path).map_err(|_| "TRUSTED_FILE_CHANGED")? != physical {
            return Err("TRUSTED_FILE_CHANGED");
        }
        Ok(Self {
            path: path.to_owned(),
            physical,
            identity: Identity::of(&after),
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

fn stamp(meta: &Metadata) -> (u64, u64, u64, i64, i64, i64, i64) {
    (
        meta.dev(),
        meta.ino(),
        meta.len(),
        meta.mtime(),
        meta.mtime_nsec(),
        meta.ctime(),
        meta.ctime_nsec(),
    )
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
    pub fn sole_certificate(&self) -> Result<Option<&str>> {
        Ok(self
            .shared
            .as_ref()
            .ok_or("SHARED_SOURCE_NOT_ENABLED")?
            .sole_certificate())
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

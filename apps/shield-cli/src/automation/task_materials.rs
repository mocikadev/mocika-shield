//! 私有材料包候选：固定角色、流式身份核对及无密码索引，不解释配置或执行工具。
use super::snapshot::FileSnapshot;
use rustix::fs::{openat, Mode, OFlags, CWD};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Role {
    Input,
    Config,
    Keystore,
    Apktool,
    Apksigner,
    Resources,
}

const ROLES: [Role; 6] = [
    Role::Input,
    Role::Config,
    Role::Keystore,
    Role::Apktool,
    Role::Apksigner,
    Role::Resources,
];
const SHARED_ROLES: [Role; 5] = [
    Role::Input,
    Role::Keystore,
    Role::Apktool,
    Role::Apksigner,
    Role::Resources,
];

fn roles(version: u32, has_config: bool) -> Result<&'static [Role], MaterialError> {
    match (version, has_config) {
        (1, true) | (2, true) => Ok(&ROLES),
        (2, false) => Ok(&SHARED_ROLES),
        _ => Err(MaterialError::InvalidBundle),
    }
}

impl Role {
    fn name(self) -> &'static str {
        match self {
            Self::Input => "input",
            Self::Config => "config",
            Self::Keystore => "keystore",
            Self::Apktool => "apktool",
            Self::Apksigner => "apksigner",
            Self::Resources => "resources",
        }
    }
    fn filename(self) -> &'static str {
        match self {
            Self::Input => "input.apk",
            Self::Config => "source-config.toml",
            Self::Keystore => "keystore.bin",
            Self::Apktool => "apktool.jar",
            Self::Apksigner => "apksigner.jar",
            Self::Resources => "resources.zip",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum MaterialError {
    InvalidSet,
    SourceChanged,
    InvalidBundle,
    Interrupted,
    Io(io::ErrorKind),
}

impl From<io::Error> for MaterialError {
    fn from(error: io::Error) -> Self {
        Self::Io(error.kind())
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Entry {
    role: Role,
    bytes: u64,
    sha256: [u8; 32],
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    format_version: u32,
    entries: Vec<Entry>,
}

pub(super) struct MaterialBundle {
    root: PathBuf,
    digest: [u8; 32],
}

impl MaterialBundle {
    pub fn create(
        parent: &Path,
        sources: &[(Role, &FileSnapshot)],
        checkpoint: impl FnMut() -> Result<(), MaterialError>,
    ) -> Result<Self, MaterialError> {
        Self::create_version(parent, sources, 1, checkpoint)
    }

    #[cfg(any(test, all(feature = "agent-executor", target_os = "macos")))]
    pub fn create_shared(
        parent: &Path,
        sources: &[(Role, &FileSnapshot)],
        checkpoint: impl FnMut() -> Result<(), MaterialError>,
    ) -> Result<Self, MaterialError> {
        Self::create_version(parent, sources, 2, checkpoint)
    }

    fn create_version(
        parent: &Path,
        sources: &[(Role, &FileSnapshot)],
        version: u32,
        mut checkpoint: impl FnMut() -> Result<(), MaterialError>,
    ) -> Result<Self, MaterialError> {
        validate_directory(parent)?;
        let roles = roles(
            version,
            sources.iter().any(|(role, _)| *role == Role::Config),
        )
        .map_err(|_| MaterialError::InvalidSet)?;
        if sources.len() != roles.len()
            || roles
                .iter()
                .any(|role| sources.iter().filter(|(found, _)| found == role).count() != 1)
            || sources
                .iter()
                .any(|(role, source)| source.role != role.name())
        {
            return Err(MaterialError::InvalidSet);
        }
        checkpoint()?;
        let root = fs::canonicalize(parent)?.join("materials");
        fs::DirBuilder::new().mode(0o700).create(&root)?;
        let mut entries = Vec::new();
        for &role in roles {
            let snapshot = sources
                .iter()
                .find(|(found, _)| *found == role)
                .ok_or(MaterialError::InvalidSet)?
                .1;
            if snapshot.role != role.name() {
                return Err(MaterialError::InvalidSet);
            }
            if fs::canonicalize(&snapshot.lookup_path)? != snapshot.path {
                return Err(MaterialError::SourceChanged);
            }
            let mut source = regular(&snapshot.path)?;
            if source.metadata()?.len() != snapshot.size {
                return Err(MaterialError::SourceChanged);
            }
            let mut output = OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(root.join(role.filename()))?;
            let (bytes, hash) = stream(&mut source, snapshot.size, &mut checkpoint, |chunk| {
                output.write_all(chunk).map_err(Into::into)
            })?;
            let actual_hex: String = hash.iter().map(|byte| format!("{byte:02x}")).collect();
            if bytes != snapshot.size
                || actual_hex != snapshot.sha256
                || fs::canonicalize(&snapshot.lookup_path)? != snapshot.path
            {
                return Err(MaterialError::SourceChanged);
            }
            output.sync_all()?;
            entries.push(Entry {
                role,
                bytes,
                sha256: hash,
            });
        }
        checkpoint()?;
        let manifest = Manifest {
            format_version: version,
            entries,
        };
        let bytes = serde_json::to_vec(&manifest).map_err(|_| MaterialError::InvalidBundle)?;
        let mut index = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(root.join("materials.json"))?;
        index.write_all(&bytes)?;
        index.sync_all()?;
        File::open(&root)?.sync_all()?;
        Ok(Self {
            root,
            digest: Sha256::digest(bytes).into(),
        })
    }

    /// 原配置只作身份材料，不能在暂存位置重新解析相对路径；任务解析器须显式绑定固定角色。
    pub fn path(&self, role: Role) -> PathBuf {
        self.root.join(role.filename())
    }
    pub fn digest(&self) -> [u8; 32] {
        self.digest
    }

    #[cfg(all(feature = "agent-executor", target_os = "macos"))]
    pub fn verify_sources(
        root: &Path,
        sources: &[(Role, &FileSnapshot)],
        checkpoint: impl FnMut() -> Result<(), MaterialError>,
    ) -> Result<Self, MaterialError> {
        Self::verify_sources_version(root, sources, 1, checkpoint)
    }

    #[cfg(any(test, all(feature = "agent-executor", target_os = "macos")))]
    pub fn verify_shared_sources(
        root: &Path,
        sources: &[(Role, &FileSnapshot)],
        checkpoint: impl FnMut() -> Result<(), MaterialError>,
    ) -> Result<Self, MaterialError> {
        Self::verify_sources_version(root, sources, 2, checkpoint)
    }

    #[cfg(any(test, all(feature = "agent-executor", target_os = "macos")))]
    fn verify_sources_version(
        root: &Path,
        sources: &[(Role, &FileSnapshot)],
        version: u32,
        checkpoint: impl FnMut() -> Result<(), MaterialError>,
    ) -> Result<Self, MaterialError> {
        validate_directory(root)?;
        let file = regular(&root.join("materials.json"))?;
        validate_private_file(&file)?;
        let mut bytes = Vec::new();
        file.take(4097).read_to_end(&mut bytes)?;
        if bytes.len() > 4096 {
            return Err(MaterialError::InvalidBundle);
        }
        let manifest: Manifest =
            serde_json::from_slice(&bytes).map_err(|_| MaterialError::InvalidBundle)?;
        let expected = roles(
            version,
            sources.iter().any(|(role, _)| *role == Role::Config),
        )?;
        if manifest.format_version != version
            || sources.len() != expected.len()
            || manifest
                .entries
                .iter()
                .map(|entry| entry.role)
                .ne(expected.iter().copied())
            || expected
                .iter()
                .any(|role| sources.iter().filter(|(found, _)| found == role).count() != 1)
        {
            return Err(MaterialError::InvalidBundle);
        }
        for entry in manifest.entries {
            let source = sources
                .iter()
                .find(|(role, _)| *role == entry.role)
                .ok_or(MaterialError::InvalidSet)?
                .1;
            let hash: String = entry
                .sha256
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect();
            if source.role != entry.role.name()
                || source.size != entry.bytes
                || source.sha256 != hash
            {
                return Err(MaterialError::InvalidBundle);
            }
        }
        Self::verify(root, Sha256::digest(bytes).into(), checkpoint)
    }

    pub fn verify(
        root: &Path,
        expected_digest: [u8; 32],
        mut checkpoint: impl FnMut() -> Result<(), MaterialError>,
    ) -> Result<Self, MaterialError> {
        validate_directory(root)?;
        checkpoint()?;
        let index = regular(&root.join("materials.json"))?;
        validate_private_file(&index)?;
        let mut bytes = Vec::new();
        index.take(4097).read_to_end(&mut bytes)?;
        if bytes.len() > 4096 || <[u8; 32]>::from(Sha256::digest(&bytes)) != expected_digest {
            return Err(MaterialError::InvalidBundle);
        }
        let manifest: Manifest =
            serde_json::from_slice(&bytes).map_err(|_| MaterialError::InvalidBundle)?;
        let expected = roles(
            manifest.format_version,
            manifest
                .entries
                .iter()
                .any(|entry| entry.role == Role::Config),
        )?;
        if manifest
            .entries
            .iter()
            .map(|entry| entry.role)
            .ne(expected.iter().copied())
        {
            return Err(MaterialError::InvalidBundle);
        }
        for entry in manifest.entries {
            let mut file = regular(&root.join(entry.role.filename()))?;
            validate_private_file(&file)?;
            let (bytes, hash) = stream(&mut file, entry.bytes, &mut checkpoint, |_| Ok(()))?;
            if bytes != entry.bytes || hash != entry.sha256 {
                return Err(MaterialError::InvalidBundle);
            }
        }
        Ok(Self {
            root: root.to_owned(),
            digest: expected_digest,
        })
    }
}

pub(super) fn validate_directory(path: &Path) -> Result<(), MaterialError> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_dir()
        || metadata.permissions().mode() & 0o077 != 0
        || metadata.uid() != rustix::process::geteuid().as_raw()
    {
        return Err(MaterialError::InvalidBundle);
    }
    Ok(())
}

fn regular(path: &Path) -> Result<File, MaterialError> {
    let file = File::from(
        openat(
            CWD,
            path,
            OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
            Mode::empty(),
        )
        .map_err(io::Error::from)?,
    );
    if !file.metadata()?.is_file() {
        return Err(MaterialError::InvalidBundle);
    }
    Ok(file)
}

fn validate_private_file(file: &File) -> Result<(), MaterialError> {
    let metadata = file.metadata()?;
    if metadata.nlink() != 1
        || metadata.permissions().mode() & 0o077 != 0
        || metadata.uid() != rustix::process::geteuid().as_raw()
    {
        return Err(MaterialError::InvalidBundle);
    }
    Ok(())
}

fn stream(
    file: &mut File,
    limit: u64,
    checkpoint: &mut impl FnMut() -> Result<(), MaterialError>,
    mut consume: impl FnMut(&[u8]) -> Result<(), MaterialError>,
) -> Result<(u64, [u8; 32]), MaterialError> {
    let mut input = file.take(limit.saturating_add(1));
    let mut buffer = [0u8; 64 * 1024];
    let mut hash = Sha256::new();
    let mut total = 0;
    loop {
        checkpoint()?;
        let count = input.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        total += count as u64;
        if total > limit {
            return Err(MaterialError::SourceChanged);
        }
        consume(&buffer[..count])?;
        hash.update(&buffer[..count]);
    }
    Ok((total, hash.finalize().into()))
}

#[cfg(test)]
#[path = "task_materials_tests.rs"]
mod tests;

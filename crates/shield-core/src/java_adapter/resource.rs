//! 辅助 JAR 的结构与兼容检查；摘要不是发布签名，任务身份仍归调用方。
use super::{invalid, PROTOCOL};
use anyhow::Result;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{
    collections::HashSet,
    fs,
    io::{Cursor, Read},
    path::{Path, PathBuf},
};

const MAIN: &str = "dev.mocika.shield.tools.AdapterMain";
const PREFIX: &str = "dev/mocika/shield/tools/";
const MANIFEST: &str = "META-INF/MANIFEST.MF";
const METADATA: &str = "META-INF/adapter.json";
const MAX: usize = 256 * 1024;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Source {
    name: String,
    sha256: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Class {
    name: String,
    bytes: usize,
    sha256: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Metadata {
    protocol_version: u32,
    application_version: String,
    main_class: String,
    sources: Vec<Source>,
    classes: Vec<Class>,
}

pub(super) struct Resource {
    path: PathBuf,
    pub digest: [u8; 32],
}

impl Resource {
    pub fn open(path: &Path) -> Result<Self> {
        let data = bytes(path)?;
        validate(&data)?;
        Ok(Self {
            path: path.to_owned(),
            digest: Sha256::digest(&data).into(),
        })
    }

    pub fn snapshot(&self) -> Result<Vec<u8>> {
        let data = bytes(&self.path)?;
        if <[u8; 32]>::from(Sha256::digest(&data)) != self.digest {
            return Err(invalid());
        }
        Ok(data)
    }
}

fn bytes(path: &Path) -> Result<Vec<u8>> {
    let metadata = fs::symlink_metadata(path).map_err(|_| invalid())?;
    if !metadata.is_file() || metadata.len() > MAX as u64 {
        return Err(invalid());
    }
    let mut data = Vec::new();
    fs::File::open(path)
        .map_err(|_| invalid())?
        .take(MAX as u64 + 1)
        .read_to_end(&mut data)
        .map_err(|_| invalid())?;
    if data.len() > MAX {
        return Err(invalid());
    }
    Ok(data)
}

fn hex(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn name(value: &str, suffix: &str) -> bool {
    value.starts_with(PREFIX)
        && value.ends_with(suffix)
        && value.is_ascii()
        && !value.contains("..")
        && !value.contains('\\')
}

fn validate(data: &[u8]) -> Result<()> {
    let mut archive = zip::ZipArchive::new(Cursor::new(data)).map_err(|_| invalid())?;
    if archive.len() > 20 {
        return Err(invalid());
    }
    let mut names = HashSet::new();
    let mut total = 0u64;
    for index in 0..archive.len() {
        let item = archive.by_index(index).map_err(|_| invalid())?;
        total = total.checked_add(item.size()).ok_or_else(invalid)?;
        if total > MAX as u64 || !names.insert(item.name().to_owned()) {
            return Err(invalid());
        }
    }
    let read = |archive: &mut zip::ZipArchive<Cursor<&[u8]>>, name: &str| -> Result<Vec<u8>> {
        let mut value = Vec::new();
        archive
            .by_name(name)
            .map_err(|_| invalid())?
            .take(MAX as u64 + 1)
            .read_to_end(&mut value)
            .map_err(|_| invalid())?;
        if value.len() > MAX {
            return Err(invalid());
        }
        Ok(value)
    };
    let expected = format!("Manifest-Version: 1.0\r\nMain-Class: {MAIN}\r\nImplementation-Version: {}\r\nMocika-Adapter-Protocol: {PROTOCOL}\r\n\r\n", env!("CARGO_PKG_VERSION"));
    if read(&mut archive, MANIFEST)? != expected.as_bytes() {
        return Err(invalid());
    }
    let metadata: Metadata =
        serde_json::from_slice(&read(&mut archive, METADATA)?).map_err(|_| invalid())?;
    if metadata.protocol_version != PROTOCOL
        || metadata.application_version != env!("CARGO_PKG_VERSION")
        || metadata.main_class != MAIN
        || metadata.sources.is_empty()
        || metadata.sources.len() > 16
        || metadata.classes.is_empty()
        || metadata.classes.len() > 18
    {
        return Err(invalid());
    }
    let mut sources = HashSet::new();
    for source in metadata.sources {
        if !name(&source.name, ".java") || !hex(&source.sha256) || !sources.insert(source.name) {
            return Err(invalid());
        }
    }
    let mut expected = HashSet::from([MANIFEST.to_owned(), METADATA.to_owned()]);
    for class in metadata.classes {
        if !name(&class.name, ".class")
            || !hex(&class.sha256)
            || class.bytes > MAX
            || !expected.insert(class.name.clone())
        {
            return Err(invalid());
        }
        let value = read(&mut archive, &class.name)?;
        if value.len() != class.bytes
            || value.get(..8) != Some(&b"\xca\xfe\xba\xbe\0\0\0\x34"[..])
            || format!("{:x}", Sha256::digest(&value)) != class.sha256
        {
            return Err(invalid());
        }
    }
    if expected != names
        || [
            "AdapterMain",
            "Request",
            "Reply",
            "CertificateReader",
            "CertificateCreator",
            "JarLauncher",
        ]
        .iter()
        .any(|name| !expected.contains(&format!("{PREFIX}{name}.class")))
    {
        return Err(invalid());
    }
    Ok(())
}

//! 签名内容不变量：只读 ZIP 内容摘要及当前壳的明文绑定声明，不解密、不启动 Java。
use anyhow::{bail, Context, Result};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, HashSet},
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::Path,
};

#[cfg(test)]
mod tests;

/// 不含口令或密钥。绑定是格式声明，不是 AEAD 验证或应用可运行证明。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SigningInput {
    pub content_sha256: [u8; 32],
    pub certificate_binding: Option<[u8; 32]>,
    pub entries: u64,
}

/// 排除明确的 JAR 签名元数据，保留其他业务条目；调用者可在分块读取时检查取消／期限。
/// 不验证 APK 签名，不写文件，不代表已获得执行许可。
pub fn inspect_signing_input(
    path: &Path,
    mut check: impl FnMut() -> Result<()>,
) -> Result<SigningInput> {
    check()?;
    let file = File::open(path).context("读取签名输入失败")?;
    let mut headers = file.try_clone().context("读取 ZIP 索引失败")?;
    let mut archive = zip::ZipArchive::new(file).context("签名输入不是有效 ZIP")?;
    let mut contents = BTreeMap::new();
    let mut names = HashSet::new();
    let mut locations = Vec::new();
    let mut binding_block = None;
    for index in 0..archive.len() {
        check()?;
        let mut entry = archive.by_index(index).context("读取 ZIP 条目失败")?;
        let name = entry.name_raw().to_vec();
        if !names.insert(name.clone()) {
            bail!("签名输入存在重复 ZIP 条目");
        }
        locations.push(entry.central_header_start());
        if entry.is_dir() || signature_metadata(&name) {
            continue;
        }
        let size = entry.size();
        let mut reader = DigestReader {
            inner: &mut entry,
            digest: Sha256::new(),
            bytes: 0,
        };
        if name == b"classes.dex" {
            let scan = crate::mshd::scan(&mut reader, size, false, &mut check)?;
            if scan.ambiguous || scan.damaged {
                bail!("加固尾块声明损坏或存在歧义");
            }
            binding_block = scan.block;
        } else {
            let mut chunk = [0u8; 64 * 1024];
            loop {
                check()?;
                if reader.read(&mut chunk)? == 0 {
                    break;
                }
            }
        }
        if reader.bytes != size {
            bail!("ZIP 条目实际长度与声明不一致");
        }
        contents.insert(name, (size, <[u8; 32]>::from(reader.digest.finalize())));
    }
    if !contents.contains_key(b"AndroidManifest.xml".as_slice()) {
        bail!("签名输入缺少 AndroidManifest.xml");
    }
    // ZIP 库按名称索引，可能折叠同名记录；核对中央目录连续性，不能仅依赖 len 去重。
    locations.sort_unstable();
    let mut expected = archive.central_directory_start();
    for start in locations {
        check()?;
        if start != expected {
            bail!("ZIP 中央目录存在重名或遗漏条目");
        }
        headers.seek(SeekFrom::Start(start))?;
        let mut header = [0u8; 46];
        headers.read_exact(&mut header)?;
        if &header[..4] != b"PK\x01\x02" {
            bail!("ZIP 中央目录头无效");
        }
        let extra = [28usize, 30, 32]
            .into_iter()
            .map(|at| u64::from(u16::from_le_bytes([header[at], header[at + 1]])))
            .sum::<u64>();
        expected = start
            .checked_add(46 + extra)
            .context("ZIP 中央目录长度溢出")?;
    }
    let certificate_binding = match binding_block {
        Some(block) => {
            let mut dex = archive.by_name("classes.dex")?;
            skip(&mut dex, block.offset + 8, &mut check)?;
            let mut prefix = [0u8; 552];
            let length = usize::try_from(block.bytes)?.min(prefix.len());
            dex.read_exact(&mut prefix[..length])
                .context("DEXB 明文头截断")?;
            Some(binding(&prefix[..length], u64::from(block.bytes))?)
        }
        None => None,
    };
    let mut digest = Sha256::new();
    digest.update(b"mocika-shield/signing-content/v1\0");
    for (name, (size, bytes)) in &contents {
        check()?;
        digest.update((name.len() as u64).to_le_bytes());
        digest.update(name);
        digest.update(size.to_le_bytes());
        digest.update(bytes);
    }
    check()?;
    Ok(SigningInput {
        content_sha256: digest.finalize().into(),
        certificate_binding,
        entries: contents.len() as u64,
    })
}

fn signature_metadata(name: &[u8]) -> bool {
    let name = name.to_ascii_uppercase();
    let Some(tail) = name.strip_prefix(b"META-INF/") else {
        return false;
    };
    !tail.contains(&b'/')
        && (tail == b"MANIFEST.MF"
            || tail.starts_with(b"SIG-")
            || [b".SF".as_slice(), b".RSA", b".DSA", b".EC"]
                .iter()
                .any(|suffix| tail.ends_with(suffix)))
}

fn binding(prefix: &[u8], payload_bytes: u64) -> Result<[u8; 32]> {
    if prefix.len() < 13 || &prefix[..4] != b"DEXB" {
        bail!("加固载荷头无效");
    }
    if u32::from_le_bytes(prefix[4..8].try_into()?) != 5 {
        bail!("独立签名不支持该加固载荷版本");
    }
    if u32::from_le_bytes(prefix[8..12].try_into()?) == 0 || prefix[12] != 64 {
        bail!("加固载荷计数或证书绑定无效");
    }
    let signature = prefix.get(13..77).context("证书绑定截断")?;
    let ikm_bytes = *prefix.get(77).context("加固密钥材料头截断")?;
    let header_bytes = 78u64 + u64::from(ikm_bytes) + 12;
    if ikm_bytes == 0 || payload_bytes < header_bytes + 16 || (prefix.len() as u64) < header_bytes {
        bail!("加固载荷头或认证标签不完整");
    }
    let mut digest = [0u8; 32];
    for (index, pair) in signature.as_chunks::<2>().0.iter().enumerate() {
        let text = std::str::from_utf8(pair).context("证书绑定不是十六进制")?;
        if !pair.iter().all(u8::is_ascii_hexdigit) {
            bail!("证书绑定不是十六进制");
        }
        digest[index] = u8::from_str_radix(text, 16)?;
    }
    Ok(digest)
}

fn skip(
    reader: &mut impl Read,
    mut bytes: u64,
    check: &mut impl FnMut() -> Result<()>,
) -> Result<()> {
    let mut buffer = [0u8; 64 * 1024];
    while bytes > 0 {
        check()?;
        let length = usize::try_from(bytes.min(buffer.len() as u64))?;
        let count = reader.read(&mut buffer[..length])?;
        if count == 0 {
            bail!("加固载荷偏移截断");
        }
        bytes -= count as u64;
    }
    Ok(())
}

struct DigestReader<R> {
    inner: R,
    digest: Sha256,
    bytes: u64,
}
impl<R: Read> Read for DigestReader<R> {
    fn read(&mut self, output: &mut [u8]) -> std::io::Result<usize> {
        let count = self.inner.read(output)?;
        self.digest.update(&output[..count]);
        self.bytes = self
            .bytes
            .checked_add(count as u64)
            .ok_or_else(|| std::io::Error::other("ZIP 条目长度溢出"))?;
        Ok(count)
    }
}

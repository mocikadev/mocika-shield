use anyhow::{Context, Result};
use rand::RngCore;
use std::fs;
use std::io::Write;
use std::path::Path;

use super::crypto;

struct DexMeta {
    name: String,
    original_size: u32,
    compressed_size: u32,
    data: Vec<u8>,
}

pub struct DexPacker {
    dex_files: Vec<DexMeta>,
    total_original_size: usize,
    total_compressed_size: usize,
}

impl DexPacker {
    pub fn new() -> Self {
        Self {
            dex_files: Vec::new(),
            total_original_size: 0,
            total_compressed_size: 0,
        }
    }

    pub fn add_dex(&mut self, dex_path: &Path, name: &str) -> Result<()> {
        let dex_data =
            fs::read(dex_path).with_context(|| format!("无法读取文件: {:?}", dex_path))?;
        let file_size = dex_data.len();

        let compressed = zstd::encode_all(&dex_data[..], 19).context("Zstd 压缩失败")?;
        let compressed_size = compressed.len();

        self.total_original_size += file_size;
        self.total_compressed_size += compressed_size;

        let ratio = 100.0 * compressed_size as f32 / file_size as f32;
        println!(
            "  ✓ {}: {} -> {} bytes ({:.1}%)",
            name, file_size, compressed_size, ratio
        );

        self.dex_files.push(DexMeta {
            name: name.to_string(),
            original_size: file_size as u32,
            compressed_size: compressed_size as u32,
            data: compressed,
        });
        Ok(())
    }

    pub fn pack(&self, output_path: &Path, ikm: &[u8], signature: &str) -> Result<()> {
        if ikm.is_empty() {
            anyhow::bail!("IKM 不能为空");
        }
        if self.dex_files.is_empty() {
            anyhow::bail!("没有 DEX 文件可打包");
        }
        let sig_bytes = signature.as_bytes();
        if sig_bytes.len() > u8::MAX as usize {
            anyhow::bail!("签名指纹长度不能超过255字节");
        }
        if ikm.len() > u8::MAX as usize {
            anyhow::bail!("IKM 长度不能超过255字节");
        }

        let mut nonce = [0u8; 12];
        rand::rng().fill_bytes(&mut nonce);
        // HKDF info 字段传入签名指纹，将密钥与 APK 证书绑定
        let derived_key = crypto::derive_key(ikm, &nonce, sig_bytes);

        let mut plaintext = Vec::new();
        for meta in &self.dex_files {
            let name_bytes = meta.name.as_bytes();
            plaintext.push(name_bytes.len() as u8);
            plaintext.extend_from_slice(name_bytes);
            plaintext.extend_from_slice(&meta.compressed_size.to_le_bytes());
            plaintext.extend_from_slice(&meta.original_size.to_le_bytes());
        }
        for meta in &self.dex_files {
            plaintext.extend_from_slice(&meta.data);
        }

        let ciphertext = crypto::encrypt(&plaintext, &derived_key, &nonce);

        let dex_count = self.dex_files.len() as u32;
        // DEXB v5 头部明文区：magic(4) + version(4) + dex_count(4)
        //   + sig_len(1) + signature[sig_len]
        //   + ikm_len(1) + ikm[ikm_len]
        //   + nonce(12) → 密文
        let mut out = Vec::with_capacity(
            4 + 4 + 4 + 1 + sig_bytes.len() + 1 + ikm.len() + 12 + ciphertext.len(),
        );
        out.extend_from_slice(b"DEXB");
        out.extend_from_slice(&5u32.to_le_bytes());
        out.extend_from_slice(&dex_count.to_le_bytes());
        out.push(sig_bytes.len() as u8);
        out.extend_from_slice(sig_bytes);
        out.push(ikm.len() as u8);
        out.extend_from_slice(ikm);
        out.extend_from_slice(&nonce);
        out.extend_from_slice(&ciphertext);

        let mut file = fs::File::create(output_path)
            .with_context(|| format!("无法创建输出文件: {:?}", output_path))?;
        file.write_all(&out).context("写入文件失败")?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_fake_dex(content: &[u8]) -> tempfile::NamedTempFile {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        f.write_all(content).unwrap();
        f
    }

    fn parse_bin(raw: &[u8], ikm: &[u8], signature: &str) -> (u32, u32, Vec<(String, u32, u32)>) {
        use chacha20poly1305::aead::{Aead, KeyInit};

        assert_eq!(&raw[0..4], b"DEXB", "magic 不匹配");
        let version = u32::from_le_bytes(raw[4..8].try_into().unwrap());
        assert_eq!(version, 5, "version 应为 5");
        let dex_count = u32::from_le_bytes(raw[8..12].try_into().unwrap());
        let sig_len = raw[12] as usize;
        let ikm_offset = 13 + sig_len;
        let ikm_len = raw[ikm_offset] as usize;
        let nonce_offset = ikm_offset + 1 + ikm_len;
        let nonce: [u8; 12] = raw[nonce_offset..nonce_offset + 12].try_into().unwrap();

        let derived_key = crypto::derive_key(ikm, &nonce, signature.as_bytes());
        let cipher = chacha20poly1305::ChaCha20Poly1305::new((&derived_key).into());
        let n = chacha20poly1305::Nonce::from_slice(&nonce);
        let plaintext = cipher
            .decrypt(n, &raw[nonce_offset + 12..])
            .expect("ChaCha20 解密失败");

        let mut pos = 0;
        let mut metas = Vec::new();
        for _ in 0..dex_count {
            let name_len = plaintext[pos] as usize;
            pos += 1;
            let name = String::from_utf8(plaintext[pos..pos + name_len].to_vec()).unwrap();
            pos += name_len;
            let compressed_size = u32::from_le_bytes(plaintext[pos..pos + 4].try_into().unwrap());
            pos += 4;
            let original_size = u32::from_le_bytes(plaintext[pos..pos + 4].try_into().unwrap());
            pos += 4;
            metas.push((name, compressed_size, original_size));
        }
        (version, dex_count, metas)
    }

    #[test]
    fn single_dex_pack_format_roundtrip() {
        let fake_dex_data = vec![0xDEu8, 0xAD, 0xBE, 0xEF, 0x00, 0x00, 0x00, 0x00];
        let tmp_dex = make_fake_dex(&fake_dex_data);

        let mut packer = DexPacker::new();
        packer.add_dex(tmp_dex.path(), "classes.dex").unwrap();

        let tmp_out = tempfile::NamedTempFile::new().unwrap();
        let ikm = b"random32bytesikmmaterialhere!!!";
        let signature = "AABBCCDDEEFF00112233445566778899AABBCCDDEEFF00112233445566778899";
        packer.pack(tmp_out.path(), ikm, signature).unwrap();

        let raw = fs::read(tmp_out.path()).unwrap();
        assert!(!raw.is_empty());

        let (_, dex_count, metas) = parse_bin(&raw, ikm, signature);
        assert_eq!(dex_count, 1);
        assert_eq!(metas[0].0, "classes.dex");
        assert_eq!(
            metas[0].2,
            fake_dex_data.len() as u32,
            "original_size 不匹配"
        );
    }

    #[test]
    fn multi_dex_count_field_correct() {
        let tmp1 = make_fake_dex(b"dex1");
        let tmp2 = make_fake_dex(b"dex2-longer-data");

        let mut packer = DexPacker::new();
        packer.add_dex(tmp1.path(), "classes.dex").unwrap();
        packer.add_dex(tmp2.path(), "classes2.dex").unwrap();

        let tmp_out = tempfile::NamedTempFile::new().unwrap();
        let ikm = b"random32bytesikmmaterialhere!!!";
        let signature = "AABBCCDDEEFF00112233445566778899AABBCCDDEEFF00112233445566778899";
        packer.pack(tmp_out.path(), ikm, signature).unwrap();

        let raw = fs::read(tmp_out.path()).unwrap();
        let (_, dex_count, metas) = parse_bin(&raw, ikm, signature);
        assert_eq!(dex_count, 2);
        assert_eq!(metas[0].0, "classes.dex");
        assert_eq!(metas[1].0, "classes2.dex");
    }

    #[test]
    fn empty_ikm_returns_error() {
        let tmp_dex = make_fake_dex(b"data");
        let mut packer = DexPacker::new();
        packer.add_dex(tmp_dex.path(), "classes.dex").unwrap();

        let tmp_out = tempfile::NamedTempFile::new().unwrap();
        assert!(packer.pack(tmp_out.path(), b"", "").is_err());
    }

    #[test]
    fn no_dex_files_returns_error() {
        let packer = DexPacker::new();
        let tmp_out = tempfile::NamedTempFile::new().unwrap();
        assert!(packer.pack(tmp_out.path(), b"key", "").is_err());
    }
}

use std::io::Cursor;

use super::crypto;

type Result<T> = std::result::Result<T, String>;

const MSHD_MAGIC: &[u8; 4] = b"MSHD";
const MSHD_TRAILER_LEN: usize = 8;
const DEXB_NONCE_LEN: usize = 12;

const MAX_PAYLOAD_LEN: usize = 512 * 1024 * 1024;
const MAX_DEX_COUNT: u32 = 256;

pub fn extract_from_dex_tail(dex_bytes: &[u8]) -> Result<&[u8]> {
    if dex_bytes.len() < MSHD_TRAILER_LEN {
        return Err("classes.dex 过短，不含 MSHD 数据".to_string());
    }

    let trailer_start = dex_bytes.len() - MSHD_TRAILER_LEN;
    if validate_trailer(dex_bytes, trailer_start).is_ok() {
        return extract_at(dex_bytes, trailer_start);
    }

    let found = dex_bytes
        .windows(4)
        .rposition(|w| w == MSHD_MAGIC)
        .and_then(|pos| validate_trailer(dex_bytes, pos).ok().map(|_| pos));

    let mshd_pos = found.ok_or_else(|| "classes.dex 末尾未找到有效 MSHD trailer".to_string())?;
    extract_at(dex_bytes, mshd_pos)
}

fn validate_trailer(dex_bytes: &[u8], mshd_pos: usize) -> Result<()> {
    let len_end = mshd_pos
        .checked_add(MSHD_TRAILER_LEN)
        .ok_or("MSHD header 偏移溢出")?;
    if len_end > dex_bytes.len() {
        return Err("MSHD header 越界".to_string());
    }
    if &dex_bytes[mshd_pos..mshd_pos + 4] != MSHD_MAGIC {
        return Err("magic 不匹配".to_string());
    }
    let payload_len = u32::from_le_bytes(
        dex_bytes[mshd_pos + 4..len_end]
            .try_into()
            .map_err(|_| "MSHD payload_len 字段解析失败".to_string())?,
    ) as usize;
    let payload_end = len_end
        .checked_add(payload_len)
        .ok_or("payload 长度溢出 usize")?;
    if payload_end != dex_bytes.len() {
        return Err(format!(
            "trailer 长度不一致: mshd_pos={} payload_len={} 期望文件末尾={} 实际={}",
            mshd_pos,
            payload_len,
            payload_end,
            dex_bytes.len()
        ));
    }
    if payload_len > MAX_PAYLOAD_LEN {
        return Err(format!(
            "payload 长度 {} 超过上限 {}",
            payload_len, MAX_PAYLOAD_LEN
        ));
    }
    Ok(())
}

fn extract_at(dex_bytes: &[u8], mshd_pos: usize) -> Result<&[u8]> {
    let len_end = mshd_pos + MSHD_TRAILER_LEN;
    let payload_len = u32::from_le_bytes(
        dex_bytes[mshd_pos + 4..len_end]
            .try_into()
            .map_err(|_| "MSHD payload_len 字段提取失败".to_string())?,
    ) as usize;
    Ok(&dex_bytes[len_end..len_end + payload_len])
}

pub struct ParsedPayload {
    pub expected_signature: String,
    pub ikm: Vec<u8>,
    pub nonce: [u8; DEXB_NONCE_LEN],
    pub ciphertext: Vec<u8>,
}

pub fn parse_header(bin_data: &[u8]) -> Result<ParsedPayload> {
    if bin_data.len() < 12 {
        return Err(format!(
            "数据过短: {} 字节，至少需要 12 字节基础头部",
            bin_data.len()
        ));
    }

    if &bin_data[0..4] != b"DEXB" {
        return Err("无效的 magic，期望 DEXB".to_string());
    }

    let version = u32::from_le_bytes(
        bin_data[4..8]
            .try_into()
            .map_err(|_| "version 字段解析失败".to_string())?,
    );
    let dex_count = u32::from_le_bytes(
        bin_data[8..12]
            .try_into()
            .map_err(|_| "dex_count 字段解析失败".to_string())?,
    );
    if dex_count > MAX_DEX_COUNT {
        return Err(format!(
            "dex_count {} 超过上限 {}，数据可能损坏",
            dex_count, MAX_DEX_COUNT
        ));
    }

    match version {
        4 => Err("格式版本不支持，请重新加固".to_string()),
        5 => {
            let sig_len = bin_data[12] as usize;
            let sig_start = 13usize;
            let sig_end = sig_start
                .checked_add(sig_len)
                .ok_or_else(|| "签名长度溢出".to_string())?;
            let ikm_len_offset = sig_end;
            if ikm_len_offset >= bin_data.len() {
                return Err("数据过短：缺少 ikm_len 字段".to_string());
            }
            let ikm_len = bin_data[ikm_len_offset] as usize;
            let ikm_start = ikm_len_offset + 1;
            let ikm_end = ikm_start
                .checked_add(ikm_len)
                .ok_or_else(|| "IKM 长度溢出".to_string())?;
            let nonce_end = ikm_end
                .checked_add(DEXB_NONCE_LEN)
                .ok_or_else(|| "nonce 偏移溢出".to_string())?;

            if nonce_end > bin_data.len() {
                return Err(format!(
                    "数据过短: {} 字节，至少需要 {} 字节头部",
                    bin_data.len(),
                    nonce_end
                ));
            }

            let expected_signature = String::from_utf8(bin_data[sig_start..sig_end].to_vec())
                .map_err(|_| "v5 header 中的签名不是合法 UTF-8 字符串".to_string())?;
            let ikm = bin_data[ikm_start..ikm_end].to_vec();
            let nonce: [u8; DEXB_NONCE_LEN] = bin_data[ikm_end..nonce_end]
                .try_into()
                .map_err(|_| "nonce 字段解析失败".to_string())?;

            Ok(ParsedPayload {
                expected_signature,
                ikm,
                nonce,
                ciphertext: bin_data[nonce_end..].to_vec(),
            })
        }
        _ => Err(format!("不支持的版本: {}，当前仅支持 v5", version)),
    }
}

pub fn parse_and_decompress_all(
    bin_data: &[u8],
    cert_fingerprint: &[u8],
) -> Result<(String, Vec<Vec<u8>>)> {
    if bin_data.len() < 12 {
        return Err(format!(
            "数据过短: {} 字节，至少需要 12 字节基础头部",
            bin_data.len()
        ));
    }

    if &bin_data[0..4] != b"DEXB" {
        return Err("无效的 magic，期望 DEXB".to_string());
    }

    let version = u32::from_le_bytes(
        bin_data[4..8]
            .try_into()
            .map_err(|_| "version 字段解析失败".to_string())?,
    );
    let dex_count = u32::from_le_bytes(
        bin_data[8..12]
            .try_into()
            .map_err(|_| "dex_count 字段解析失败".to_string())?,
    );
    if dex_count > MAX_DEX_COUNT {
        return Err(format!(
            "dex_count {} 超过上限 {}，数据可能损坏",
            dex_count, MAX_DEX_COUNT
        ));
    }

    match version {
        4 => Err("格式版本不支持，请重新加固".to_string()),
        5 => {
            let payload = parse_header(bin_data)?;
            let plaintext = crypto::decrypt(
                &payload.ciphertext,
                &crypto::derive_key(&payload.ikm, &payload.nonce, cert_fingerprint),
                &payload.nonce,
            )?;
            let dex_files = parse_dex_entries(&plaintext, dex_count)?;
            Ok((payload.expected_signature, dex_files))
        }
        _ => Err(format!("不支持的版本: {}，当前仅支持 v5", version)),
    }
}

fn parse_dex_entries(plaintext: &[u8], dex_count: u32) -> Result<Vec<Vec<u8>>> {
    let mut cursor = Cursor::new(plaintext);
    let mut metas = Vec::new();

    for _ in 0..dex_count {
        let pos = cursor.position() as usize;
        if pos >= plaintext.len() {
            return Err("解析 meta 时数据意外结束".to_string());
        }
        let name_len = plaintext[pos] as usize;
        cursor.set_position((pos + 1) as u64);

        let pos = cursor.position() as usize;
        if pos + name_len + 8 > plaintext.len() {
            return Err("meta 字段越界".to_string());
        }
        cursor.set_position((pos + name_len) as u64);

        let pos = cursor.position() as usize;
        let compressed_size = u32::from_le_bytes(
            plaintext[pos..pos + 4]
                .try_into()
                .map_err(|_| "DEX meta compressed_size 字段解析失败".to_string())?,
        ) as usize;
        let original_size = u32::from_le_bytes(
            plaintext[pos + 4..pos + 8]
                .try_into()
                .map_err(|_| "DEX meta original_size 字段解析失败".to_string())?,
        ) as usize;
        cursor.set_position((pos + 8) as u64);

        metas.push((compressed_size, original_size));
    }

    let mut data_offset = cursor.position() as usize;
    let mut dex_files = Vec::new();

    for (compressed_size, original_size) in metas {
        let end = data_offset
            .checked_add(compressed_size)
            .ok_or_else(|| "数据偏移溢出".to_string())?;
        if end > plaintext.len() {
            return Err(format!(
                "数据越界: offset={} compressed_size={} total={}",
                data_offset,
                compressed_size,
                plaintext.len()
            ));
        }

        let decompressed = zstd::decode_all(&plaintext[data_offset..end])
            .map_err(|e| format!("Zstd 解压失败: {}", e))?;

        if decompressed.len() != original_size {
            return Err(format!(
                "解压后大小不匹配: 期望 {}，实际 {}",
                original_size,
                decompressed.len()
            ));
        }

        dex_files.push(decompressed);
        data_offset += compressed_size;
    }

    Ok(dex_files)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chacha20poly1305::{
        aead::{Aead, KeyInit},
        ChaCha20Poly1305, Nonce,
    };

    fn build_plaintext_dex(name: &str, dex_data: &[u8]) -> Vec<u8> {
        let compressed = zstd::encode_all(dex_data, 1).unwrap();
        let mut plaintext = Vec::new();
        plaintext.push(name.len() as u8);
        plaintext.extend_from_slice(name.as_bytes());
        plaintext.extend_from_slice(&(compressed.len() as u32).to_le_bytes());
        plaintext.extend_from_slice(&(dex_data.len() as u32).to_le_bytes());
        plaintext.extend_from_slice(&compressed);
        plaintext
    }

    fn build_encrypted_payload(
        plaintext: &[u8],
        ikm: &[u8],
        nonce: [u8; DEXB_NONCE_LEN],
        fp: &[u8],
    ) -> Vec<u8> {
        let derived_key = crypto::derive_key(ikm, &nonce, fp);
        let cipher = ChaCha20Poly1305::new((&derived_key).into());
        cipher
            .encrypt(Nonce::from_slice(&nonce), plaintext)
            .unwrap()
    }

    #[test]
    fn magic_mismatch_returns_error() {
        let bad =
            b"BADD\x03\x00\x00\x00\x01\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00";
        assert!(parse_and_decompress_all(bad, b"").is_err());
    }

    #[test]
    fn unsupported_version_returns_error() {
        let mut data = vec![0u8; 24];
        data[0..4].copy_from_slice(b"DEXB");
        data[4..8].copy_from_slice(&2u32.to_le_bytes());
        assert!(parse_and_decompress_all(&data, b"").is_err());
    }

    #[test]
    fn too_short_returns_error() {
        assert!(parse_and_decompress_all(b"DEX", b"").is_err());
    }

    #[test]
    fn legacy_format_requires_reprotect() {
        let mut data = vec![0u8; 25];
        data[0..4].copy_from_slice(b"DEXB");
        data[4..8].copy_from_slice(&4u32.to_le_bytes());
        data[8..12].copy_from_slice(&1u32.to_le_bytes());
        let err = parse_and_decompress_all(&data, b"").unwrap_err();
        assert!(err.contains("重新加固"), "错误信息应提示重新加固: {}", err);
    }

    #[test]
    fn v5_roundtrip_with_signature_binding() {
        let ikm = b"random-ikm-32-bytes-for-testing!";
        let nonce = [3u8; DEXB_NONCE_LEN];
        let signature = "AABBCCDDEEFF00112233445566778899AABBCCDDEEFF00112233445566778899";
        let dex_data = b"dex-v5-content";
        let plaintext = build_plaintext_dex("classes.dex", dex_data);
        let ciphertext = build_encrypted_payload(&plaintext, ikm, nonce, signature.as_bytes());

        let mut bin = Vec::new();
        bin.extend_from_slice(b"DEXB");
        bin.extend_from_slice(&5u32.to_le_bytes());
        bin.extend_from_slice(&1u32.to_le_bytes());
        bin.push(signature.len() as u8);
        bin.extend_from_slice(signature.as_bytes());
        bin.push(ikm.len() as u8);
        bin.extend_from_slice(ikm);
        bin.extend_from_slice(&nonce);
        bin.extend_from_slice(&ciphertext);

        let (expected_sig, dex_files) =
            parse_and_decompress_all(&bin, signature.as_bytes()).unwrap();
        assert_eq!(expected_sig, signature);
        assert_eq!(dex_files, vec![dex_data.to_vec()]);
    }

    #[test]
    fn v5_wrong_fingerprint_fails_decryption() {
        let ikm = b"random-ikm-32-bytes-for-testing!";
        let nonce = [4u8; DEXB_NONCE_LEN];
        let signature = "AABBCCDDEEFF00112233445566778899AABBCCDDEEFF00112233445566778899";
        let dex_data = b"content";
        let plaintext = build_plaintext_dex("classes.dex", dex_data);
        let ciphertext = build_encrypted_payload(&plaintext, ikm, nonce, signature.as_bytes());

        let mut bin = Vec::new();
        bin.extend_from_slice(b"DEXB");
        bin.extend_from_slice(&5u32.to_le_bytes());
        bin.extend_from_slice(&1u32.to_le_bytes());
        bin.push(signature.len() as u8);
        bin.extend_from_slice(signature.as_bytes());
        bin.push(ikm.len() as u8);
        bin.extend_from_slice(ikm);
        bin.extend_from_slice(&nonce);
        bin.extend_from_slice(&ciphertext);

        assert!(parse_and_decompress_all(&bin, b"WRONG_FINGERPRINT").is_err());
    }

    #[test]
    fn extract_from_dex_tail_extracts_payload() {
        let payload = b"DEXB_FAKE_PAYLOAD";
        let payload_len = payload.len() as u32;
        let mut dex = b"DEX_STUB_CONTENT_".to_vec();
        dex.extend_from_slice(b"MSHD");
        dex.extend_from_slice(&payload_len.to_le_bytes());
        dex.extend_from_slice(payload);

        let extracted = extract_from_dex_tail(&dex).unwrap();
        assert_eq!(extracted, payload);
    }

    #[test]
    fn extract_from_dex_tail_no_magic_returns_error() {
        let dex = b"DEX_CONTENT_WITHOUT_MAGIC_AT_ALL____";
        assert!(extract_from_dex_tail(dex).is_err());
    }

    #[test]
    fn extract_from_dex_tail_overflow_payload_returns_error() {
        let mut dex = b"DEX_STUB_".to_vec();
        dex.extend_from_slice(b"MSHD");
        dex.extend_from_slice(&9999u32.to_le_bytes());
        assert!(extract_from_dex_tail(&dex).is_err());
    }
}

//! 保持既有 enc:v1 格式和派生上下文；模块迁移不改变旧记录的解密条件。
use super::CertificateRecord;
use chacha20poly1305::{
    aead::{Aead, KeyInit},
    ChaCha20Poly1305, Nonce,
};
use rand::RngCore;
use sha2::{Digest, Sha256};
use std::{env, path::Path};
const SECRET_PREFIX: &str = "enc:v1:";

pub(super) fn derive_crypto_key(db_path: &Path, keystore_dir: &Path) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(b"mocika-shield-gui-cert-store-v1");
    hasher.update(db_path.to_string_lossy().as_bytes());
    hasher.update(keystore_dir.to_string_lossy().as_bytes());
    if let Some(home) = env::var_os("HOME").or_else(|| env::var_os("USERPROFILE")) {
        hasher.update(home.to_string_lossy().as_bytes());
    }
    if let Some(user) = env::var_os("USER").or_else(|| env::var_os("USERNAME")) {
        hasher.update(user.to_string_lossy().as_bytes());
    }
    hasher.finalize().into()
}

pub(super) fn encrypt_secret(key: &[u8; 32], value: &str) -> Result<String, String> {
    let cipher = ChaCha20Poly1305::new_from_slice(key)
        .map_err(|_| "初始化证书密码加密器失败".to_string())?;
    let mut nonce = [0u8; 12];
    rand::rng().fill_bytes(&mut nonce);
    let ciphertext = cipher
        .encrypt(Nonce::from_slice(&nonce), value.as_bytes())
        .map_err(|_| "加密证书密码失败".to_string())?;
    Ok(format!(
        "{}{}:{}",
        SECRET_PREFIX,
        encode_hex(&nonce),
        encode_hex(&ciphertext)
    ))
}

fn decrypt_secret(key: &[u8; 32], value: &str) -> Result<String, String> {
    let rest = value
        .strip_prefix(SECRET_PREFIX)
        .ok_or_else(|| "证书数据库包含未加密密码，请重新导入或创建证书".to_string())?;
    let (nonce_hex, cipher_hex) = rest
        .split_once(':')
        .ok_or_else(|| "证书数据库密码字段格式无效".to_string())?;
    let nonce = decode_hex(nonce_hex)?;
    if nonce.len() != 12 {
        return Err("证书数据库密码 nonce 长度无效".to_string());
    }
    let ciphertext = decode_hex(cipher_hex)?;
    let cipher = ChaCha20Poly1305::new_from_slice(key)
        .map_err(|_| "初始化证书密码解密器失败".to_string())?;
    let plaintext = cipher
        .decrypt(Nonce::from_slice(&nonce), ciphertext.as_ref())
        .map_err(|_| "解密证书密码失败，请重新导入或创建证书".to_string())?;
    String::from_utf8(plaintext).map_err(|_| "证书密码不是合法 UTF-8".to_string())
}

pub(super) fn decrypt_record_secrets(
    key: &[u8; 32],
    item: &mut CertificateRecord,
) -> Result<(), String> {
    item.keystore_password = decrypt_secret(key, &item.keystore_password)?;
    item.key_password = decrypt_secret(key, &item.key_password)?;
    Ok(())
}

fn encode_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for &byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

fn decode_hex(value: &str) -> Result<Vec<u8>, String> {
    if !value.len().is_multiple_of(2) {
        return Err("证书数据库密码字段 hex 长度无效".to_string());
    }
    let mut out = Vec::with_capacity(value.len() / 2);
    let bytes = value.as_bytes();
    for pair in bytes.as_chunks::<2>().0 {
        let hi = decode_hex_nibble(pair[0])?;
        let lo = decode_hex_nibble(pair[1])?;
        out.push((hi << 4) | lo);
    }
    Ok(out)
}

fn decode_hex_nibble(byte: u8) -> Result<u8, String> {
    match byte {
        b'0'..=b'9' => Ok(byte - b'0'),
        b'a'..=b'f' => Ok(byte - b'a' + 10),
        b'A'..=b'F' => Ok(byte - b'A' + 10),
        _ => Err("证书数据库密码字段包含非法 hex 字符".to_string()),
    }
}

use chacha20poly1305::{
    aead::{Aead, KeyInit},
    ChaCha20Poly1305, Nonce,
};
use hkdf::Hkdf;
use sha2::Sha256;

pub fn derive_key(ikm: &[u8], nonce: &[u8; 12], cert_fingerprint: &[u8]) -> [u8; 32] {
    let hk = Hkdf::<Sha256>::new(Some(nonce), ikm);
    let mut okm = [0u8; 32];
    hk.expand(cert_fingerprint, &mut okm)
        .expect("HKDF expand 长度固定为 32，不会失败");
    okm
}

pub fn encrypt(plaintext: &[u8], key: &[u8; 32], nonce: &[u8; 12]) -> Vec<u8> {
    let cipher = ChaCha20Poly1305::new(key.into());
    let nonce = Nonce::from_slice(nonce);
    cipher
        .encrypt(nonce, plaintext)
        .expect("ChaCha20-Poly1305 加密不会因内存外原因失败")
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::RngCore;

    fn random_nonce() -> [u8; 12] {
        let mut n = [0u8; 12];
        rand::rng().fill_bytes(&mut n);
        n
    }

    #[test]
    fn encrypt_decrypt_roundtrip() {
        let ikm = b"test-key-for-unit-test";
        let nonce = random_nonce();
        let fp = b"AABBCCDDEEFF00112233445566778899AABBCCDDEEFF00112233445566778899";
        let key = derive_key(ikm, &nonce, fp);
        let plaintext = b"hello mocika shield dex payload";

        let ciphertext = encrypt(plaintext, &key, &nonce);
        assert_ne!(ciphertext, plaintext);

        let cipher = chacha20poly1305::ChaCha20Poly1305::new((&key).into());
        let n = chacha20poly1305::Nonce::from_slice(&nonce);
        let decrypted = cipher.decrypt(n, ciphertext.as_ref()).unwrap();
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn tampered_ciphertext_fails_decryption() {
        let ikm = b"another-key";
        let nonce = random_nonce();
        let fp = b"AABBCCDDEEFF00112233445566778899AABBCCDDEEFF00112233445566778899";
        let key = derive_key(ikm, &nonce, fp);
        let plaintext = b"sensitive dex data";

        let mut ciphertext = encrypt(plaintext, &key, &nonce);
        ciphertext[0] ^= 0xFF;

        let cipher = chacha20poly1305::ChaCha20Poly1305::new((&key).into());
        let n = chacha20poly1305::Nonce::from_slice(&nonce);
        assert!(cipher.decrypt(n, ciphertext.as_ref()).is_err());
    }

    #[test]
    fn different_nonces_produce_different_keys() {
        let ikm = b"same-key-material";
        let fp = b"AABBCCDDEEFF00112233445566778899AABBCCDDEEFF00112233445566778899";
        let nonce1 = [0u8; 12];
        let nonce2 = [1u8; 12];
        assert_ne!(derive_key(ikm, &nonce1, fp), derive_key(ikm, &nonce2, fp));
    }

    #[test]
    fn different_fingerprints_produce_different_keys() {
        let ikm = b"same-key-material";
        let nonce = [0u8; 12];
        let fp1 = b"AABBCCDDEEFF00112233445566778899AABBCCDDEEFF00112233445566778899";
        let fp2 = b"0011223344556677889900AABBCCDDEEFF00112233445566778899AABBCCDDEE";
        assert_ne!(derive_key(ikm, &nonce, fp1), derive_key(ikm, &nonce, fp2));
    }
}

//! 仅用于研究测试的认证总索引与逐 DEX 独立密文编排。

use std::collections::HashSet;

use chacha20poly1305::{
    aead::{Aead, Payload},
    ChaCha20Poly1305, KeyInit, Nonce,
};
use sha2::{Digest, Sha256};
use thiserror::Error;

use super::{
    bundle::{open_bundle, seal_bundle, BundleCarrier, BundleError, SealedBundle},
    manifest::{GroupManifestError, GroupMember},
};

const INDEX_MAGIC: &[u8; 4] = b"DRI1";
const INDEX_VERSION: u32 = 1;
const INDEX_AAD: &[u8] = b"mocika-shield/dex-layered-index/research/v1";
const MAX_SHARD_COUNT: usize = 256;
const MAX_IDENTITY_SIZE: usize = 1024;

#[derive(Debug, Error, PartialEq, Eq)]
pub(super) enum LayeredError {
    #[error("构建单 DEX 密文失败：{0}")]
    Bundle(#[from] BundleError),
    #[error("构建成员清单失败：{0}")]
    Manifest(#[from] GroupManifestError),
    #[error("DEX 集合不能为空")]
    EmptyGroup,
    #[error("DEX 数量超过分层索引上限：limit={limit}, actual={actual}")]
    TooManyShards { limit: usize, actual: usize },
    #[error("DEX 身份过长：{actual} 字节")]
    IdentityTooLong { actual: usize },
    #[error("DEX 身份重复：{identity}")]
    DuplicateIdentity { identity: String },
    #[error("包内 nonce 重复")]
    DuplicateNonce,
    #[error("认证总索引认证失败")]
    IndexAuthenticationFailed,
    #[error("认证总索引与当前密文或载体集合不匹配")]
    IndexMismatch,
    #[error("分层成员数量不匹配：index={index}, shards={shards}, carriers={carriers}")]
    CountMismatch {
        index: usize,
        shards: usize,
        carriers: usize,
    },
    #[error("消费 DEX {identity} 失败：{message}")]
    Consumer { identity: String, message: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct LayeredPackage {
    pub(super) index_nonce: [u8; 12],
    pub(super) index_ciphertext: Vec<u8>,
    pub(super) shards: Vec<SealedBundle>,
}

pub(super) fn seal_layered(
    key: &[u8; 32],
    nonce_base: [u8; 12],
    members: &[GroupMember<'_>],
) -> Result<LayeredPackage, LayeredError> {
    validate_count(members.len())?;
    validate_member_identities(members)?;
    let index_nonce = nonce_from_base(nonce_base, 0);
    let mut shards = Vec::with_capacity(members.len());
    for (index, member) in members.iter().enumerate() {
        let nonce = nonce_from_base(nonce_base, (index + 1) as u32);
        shards.push(seal_bundle(key, nonce, std::slice::from_ref(member))?);
    }
    validate_nonces(index_nonce, &shards)?;
    let plaintext = encode_index_from_members(members, &shards)?;
    let cipher = ChaCha20Poly1305::new(key.into());
    let index_ciphertext = cipher
        .encrypt(
            Nonce::from_slice(&index_nonce),
            Payload {
                msg: &plaintext,
                aad: INDEX_AAD,
            },
        )
        .map_err(|_| LayeredError::IndexAuthenticationFailed)?;
    Ok(LayeredPackage {
        index_nonce,
        index_ciphertext,
        shards,
    })
}

pub(super) fn open_layered_each(
    key: &[u8; 32],
    package: &LayeredPackage,
    carriers: &[BundleCarrier<'_>],
    mut consume: impl FnMut(&str, Vec<u8>) -> Result<(), String>,
) -> Result<(), LayeredError> {
    if package.shards.len() != carriers.len() {
        return Err(LayeredError::CountMismatch {
            index: package.shards.len(),
            shards: package.shards.len(),
            carriers: carriers.len(),
        });
    }
    validate_count(package.shards.len())?;
    validate_nonces(package.index_nonce, &package.shards)?;
    let cipher = ChaCha20Poly1305::new(key.into());
    let authenticated = cipher
        .decrypt(
            Nonce::from_slice(&package.index_nonce),
            Payload {
                msg: &package.index_ciphertext,
                aad: INDEX_AAD,
            },
        )
        .map_err(|_| LayeredError::IndexAuthenticationFailed)?;
    let expected = encode_index_from_carriers(carriers, &package.shards)?;
    if authenticated != expected {
        return Err(LayeredError::IndexMismatch);
    }

    for ((carrier, shard), index) in carriers.iter().zip(&package.shards).zip(0usize..) {
        let restored = open_bundle(key, shard, std::slice::from_ref(carrier))?;
        let dex = restored
            .into_iter()
            .next()
            .ok_or(LayeredError::CountMismatch {
                index,
                shards: package.shards.len(),
                carriers: carriers.len(),
            })?;
        consume(carrier.identity, dex).map_err(|message| LayeredError::Consumer {
            identity: carrier.identity.to_owned(),
            message,
        })?;
    }
    Ok(())
}

fn encode_index_from_members(
    members: &[GroupMember<'_>],
    shards: &[SealedBundle],
) -> Result<Vec<u8>, LayeredError> {
    let carriers = members
        .iter()
        .map(|member| BundleCarrier::new(member.identity, &member.extraction.carrier))
        .collect::<Vec<_>>();
    encode_index_from_carriers(&carriers, shards)
}

fn encode_index_from_carriers(
    carriers: &[BundleCarrier<'_>],
    shards: &[SealedBundle],
) -> Result<Vec<u8>, LayeredError> {
    if carriers.len() != shards.len() {
        return Err(LayeredError::CountMismatch {
            index: carriers.len(),
            shards: shards.len(),
            carriers: carriers.len(),
        });
    }
    validate_count(carriers.len())?;
    validate_carrier_identities(carriers)?;
    let mut encoded = Vec::new();
    encoded.extend_from_slice(INDEX_MAGIC);
    encoded.extend_from_slice(&INDEX_VERSION.to_le_bytes());
    encoded.extend_from_slice(&(carriers.len() as u32).to_le_bytes());
    for (carrier, shard) in carriers.iter().zip(shards) {
        encoded.extend_from_slice(&(carrier.identity.len() as u16).to_le_bytes());
        encoded.extend_from_slice(carrier.identity.as_bytes());
        encoded.extend_from_slice(&Sha256::digest(carrier.carrier));
        encoded.extend_from_slice(&shard.nonce);
        encoded.extend_from_slice(&Sha256::digest(&shard.ciphertext));
    }
    Ok(encoded)
}

fn validate_count(count: usize) -> Result<(), LayeredError> {
    if count == 0 {
        return Err(LayeredError::EmptyGroup);
    }
    if count > MAX_SHARD_COUNT {
        return Err(LayeredError::TooManyShards {
            limit: MAX_SHARD_COUNT,
            actual: count,
        });
    }
    Ok(())
}

fn validate_member_identities(members: &[GroupMember<'_>]) -> Result<(), LayeredError> {
    validate_identities(members.iter().map(|member| member.identity))
}

fn validate_carrier_identities(carriers: &[BundleCarrier<'_>]) -> Result<(), LayeredError> {
    validate_identities(carriers.iter().map(|carrier| carrier.identity))
}

fn validate_identities<'a>(identities: impl Iterator<Item = &'a str>) -> Result<(), LayeredError> {
    let mut seen = HashSet::new();
    for identity in identities {
        if identity.len() > MAX_IDENTITY_SIZE || identity.len() > u16::MAX as usize {
            return Err(LayeredError::IdentityTooLong {
                actual: identity.len(),
            });
        }
        if !seen.insert(identity) {
            return Err(LayeredError::DuplicateIdentity {
                identity: identity.to_owned(),
            });
        }
    }
    Ok(())
}

fn validate_nonces(index_nonce: [u8; 12], shards: &[SealedBundle]) -> Result<(), LayeredError> {
    let mut seen = HashSet::with_capacity(shards.len() + 1);
    seen.insert(index_nonce);
    for shard in shards {
        if !seen.insert(shard.nonce) {
            return Err(LayeredError::DuplicateNonce);
        }
    }
    Ok(())
}

fn nonce_from_base(mut nonce: [u8; 12], counter: u32) -> [u8; 12] {
    let mut carry = u64::from(counter);
    for byte in &mut nonce {
        let sum = u64::from(*byte) + (carry & 0xff);
        *byte = sum as u8;
        carry = (carry >> 8) + (sum >> 8);
        if carry == 0 {
            break;
        }
    }
    nonce
}

#[cfg(test)]
mod tests {
    use super::nonce_from_base;

    #[test]
    fn nonce_基值和计数器形成稳定唯一值() {
        let base = [7u8; 12];
        assert_eq!(nonce_from_base(base, 0), base);
        assert_ne!(nonce_from_base(base, 0), nonce_from_base(base, 1));
        assert_ne!(nonce_from_base(base, 1), nonce_from_base(base, 2));
        assert_eq!(nonce_from_base([0xff; 12], 1), [0u8; 12]);
    }
}

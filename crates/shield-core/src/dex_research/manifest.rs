//! 仅用于研究测试的多 DEX 整组身份绑定与认证清单。

use std::collections::HashSet;

use chacha20poly1305::{
    aead::{Aead, Payload},
    ChaCha20Poly1305, KeyInit, Nonce,
};
use sha2::{Digest, Sha256};
use thiserror::Error;

use super::transform::{restore, CodePayload, Extraction, TransformError};

const MANIFEST_VERSION: u32 = 1;
const MANIFEST_AAD: &[u8] = b"mocika-shield/dex-group-manifest/research/v1";

#[derive(Debug, Error, PartialEq, Eq)]
pub(super) enum GroupManifestError {
    #[error("DEX 集合不能为空")]
    EmptyGroup,
    #[error("DEX 身份不能为空")]
    EmptyIdentity,
    #[error("DEX 身份过长：{length} 字节")]
    IdentityTooLong { length: usize },
    #[error("DEX 身份重复：{identity}")]
    DuplicateIdentity { identity: String },
    #[error("DEX 数量过多：{count}")]
    TooManyMembers { count: usize },
    #[error("整组清单认证失败")]
    AuthenticationFailed,
    #[error("DEX 集合与认证清单不匹配")]
    GroupMismatch,
    #[error("DEX {identity} 的原始内容与代码载荷不匹配")]
    OriginalMismatch { identity: String },
    #[error("恢复 DEX 失败：{0}")]
    Restore(#[from] TransformError),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct SealedGroupManifest {
    nonce: [u8; 12],
    pub(super) ciphertext: Vec<u8>,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct GroupMember<'a> {
    pub(super) identity: &'a str,
    pub(super) original: &'a [u8],
    pub(super) extraction: &'a Extraction,
}

impl<'a> GroupMember<'a> {
    pub(super) fn new(identity: &'a str, original: &'a [u8], extraction: &'a Extraction) -> Self {
        Self {
            identity,
            original,
            extraction,
        }
    }
}

pub(super) fn seal_group(
    key: &[u8; 32],
    nonce: [u8; 12],
    members: &[GroupMember<'_>],
) -> Result<SealedGroupManifest, GroupManifestError> {
    validate_original_pairing(members)?;
    let plaintext = encode_manifest(members)?;
    let cipher = ChaCha20Poly1305::new(key.into());
    let ciphertext = cipher
        .encrypt(
            Nonce::from_slice(&nonce),
            Payload {
                msg: &plaintext,
                aad: MANIFEST_AAD,
            },
        )
        .map_err(|_| GroupManifestError::AuthenticationFailed)?;
    Ok(SealedGroupManifest { nonce, ciphertext })
}

pub(super) fn restore_group(
    key: &[u8; 32],
    sealed: &SealedGroupManifest,
    members: &[GroupMember<'_>],
) -> Result<Vec<Vec<u8>>, GroupManifestError> {
    let cipher = ChaCha20Poly1305::new(key.into());
    let authenticated = cipher
        .decrypt(
            Nonce::from_slice(&sealed.nonce),
            Payload {
                msg: &sealed.ciphertext,
                aad: MANIFEST_AAD,
            },
        )
        .map_err(|_| GroupManifestError::AuthenticationFailed)?;
    let actual = encode_manifest(members)?;
    if authenticated != actual {
        return Err(GroupManifestError::GroupMismatch);
    }

    let restored = members
        .iter()
        .map(|member| {
            restore(&member.extraction.carrier, &member.extraction.payload).map_err(Into::into)
        })
        .collect::<Result<Vec<_>, GroupManifestError>>()?;
    for (member, dex) in members.iter().zip(&restored) {
        if dex != member.original {
            return Err(GroupManifestError::OriginalMismatch {
                identity: member.identity.to_owned(),
            });
        }
    }
    Ok(restored)
}

pub(super) fn validate_original_pairing(
    members: &[GroupMember<'_>],
) -> Result<(), GroupManifestError> {
    for member in members {
        let restored = restore(&member.extraction.carrier, &member.extraction.payload)?;
        if restored != member.original {
            return Err(GroupManifestError::OriginalMismatch {
                identity: member.identity.to_owned(),
            });
        }
    }
    Ok(())
}

pub(super) fn encode_manifest(members: &[GroupMember<'_>]) -> Result<Vec<u8>, GroupManifestError> {
    if members.is_empty() {
        return Err(GroupManifestError::EmptyGroup);
    }
    let count = u32::try_from(members.len()).map_err(|_| GroupManifestError::TooManyMembers {
        count: members.len(),
    })?;
    let mut identities = HashSet::with_capacity(members.len());
    let mut encoded = Vec::new();
    encoded.extend_from_slice(&MANIFEST_VERSION.to_le_bytes());
    encoded.extend_from_slice(&count.to_le_bytes());

    for member in members {
        if member.identity.is_empty() {
            return Err(GroupManifestError::EmptyIdentity);
        }
        if !identities.insert(member.identity) {
            return Err(GroupManifestError::DuplicateIdentity {
                identity: member.identity.to_owned(),
            });
        }
        let identity_length = u16::try_from(member.identity.len()).map_err(|_| {
            GroupManifestError::IdentityTooLong {
                length: member.identity.len(),
            }
        })?;
        encoded.extend_from_slice(&identity_length.to_le_bytes());
        encoded.extend_from_slice(member.identity.as_bytes());
        encoded.extend_from_slice(&Sha256::digest(member.original));
        encoded.extend_from_slice(&Sha256::digest(&member.extraction.carrier));
        encoded.extend_from_slice(&payload_digest(&member.extraction.payload));
    }
    Ok(encoded)
}

fn payload_digest(payload: &CodePayload) -> [u8; 32] {
    let mut digest = Sha256::new();
    digest.update(b"mocika-shield/dex-code-payload/research/v1");
    digest.update(payload.carrier_sha256);
    digest.update((payload.original_file_size as u64).to_le_bytes());
    digest.update((payload.methods.len() as u64).to_le_bytes());
    for method in &payload.methods {
        digest.update(method.method_index.to_le_bytes());
        digest.update(method.instructions_offset.to_le_bytes());
        digest.update((method.original.len() as u64).to_le_bytes());
        digest.update(&method.original);
        digest.update((method.stub.len() as u64).to_le_bytes());
        digest.update(&method.stub);
    }
    digest.finalize().into()
}

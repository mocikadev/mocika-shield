//! 仅用于研究测试的多 DEX 清单与加密代码载荷封装。

use std::collections::HashSet;

use chacha20poly1305::{
    aead::{Aead, Payload},
    ChaCha20Poly1305, KeyInit, Nonce,
};
use thiserror::Error;

use super::{
    manifest::{encode_manifest, validate_original_pairing, GroupManifestError, GroupMember},
    transform::{
        bind_stubs_from_carrier, restore, CodePayload, ExtractedMethod, Extraction, TransformError,
    },
};

const BUNDLE_MAGIC: &[u8; 4] = b"DRB1";
const BUNDLE_VERSION: u32 = 2;
const BUNDLE_AAD: &[u8] = b"mocika-shield/dex-code-bundle/research/v2";
const AEAD_TAG_SIZE: usize = 16;
const MAX_PLAINTEXT_SIZE: usize = 256 * 1024 * 1024;
const MAX_MEMBER_COUNT: usize = 256;
const MAX_METHOD_COUNT: usize = 2_000_000;
const MAX_IDENTITY_SIZE: usize = 1024;
const MAX_METHOD_CODE_SIZE: usize = 16 * 1024 * 1024;

#[derive(Debug, Error, PartialEq, Eq)]
pub(super) enum BundleError {
    #[error("构建整组清单失败：{0}")]
    Manifest(#[from] GroupManifestError),
    #[error("整组载荷字段 {field} 超过研究预算：limit={limit}, actual={actual}")]
    BudgetExceeded {
        field: &'static str,
        limit: usize,
        actual: usize,
    },
    #[error("整组载荷字段 {field} 超过编码范围：{actual}")]
    ValueTooLarge { field: &'static str, actual: usize },
    #[error("整组载荷认证失败")]
    AuthenticationFailed,
    #[error("整组载荷已截断：field={field}, needed={needed}, remaining={remaining}")]
    Truncated {
        field: &'static str,
        needed: usize,
        remaining: usize,
    },
    #[error("整组载荷 magic 无效")]
    InvalidMagic,
    #[error("整组载荷版本不受支持：{actual}")]
    UnsupportedVersion { actual: u32 },
    #[error("整组载荷包含尾随数据：{remaining} 字节")]
    TrailingData { remaining: usize },
    #[error("DEX 身份不是合法 UTF-8")]
    InvalidIdentity,
    #[error("DEX 身份重复：{identity}")]
    DuplicateIdentity { identity: String },
    #[error("载体数量不匹配：expected={expected}, actual={actual}")]
    CarrierCountMismatch { expected: usize, actual: usize },
    #[error("第 {index} 个载体身份不匹配：expected={expected}, actual={actual}")]
    CarrierIdentityMismatch {
        index: usize,
        expected: String,
        actual: String,
    },
    #[error("恢复结果与认证整组清单不匹配")]
    ManifestMismatch,
    #[error("恢复 DEX 失败：{0}")]
    Restore(#[from] TransformError),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct SealedBundle {
    pub(super) nonce: [u8; 12],
    pub(super) ciphertext: Vec<u8>,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct BundleCarrier<'a> {
    pub(super) identity: &'a str,
    pub(super) carrier: &'a [u8],
}

impl<'a> BundleCarrier<'a> {
    pub(super) fn new(identity: &'a str, carrier: &'a [u8]) -> Self {
        Self { identity, carrier }
    }
}

pub(super) fn seal_bundle(
    key: &[u8; 32],
    nonce: [u8; 12],
    members: &[GroupMember<'_>],
) -> Result<SealedBundle, BundleError> {
    validate_original_pairing(members)?;
    let manifest = encode_manifest(members)?;
    let plaintext = encode_bundle(&manifest, members)?;
    if plaintext.len() > MAX_PLAINTEXT_SIZE {
        return Err(BundleError::BudgetExceeded {
            field: "明文总长度",
            limit: MAX_PLAINTEXT_SIZE,
            actual: plaintext.len(),
        });
    }
    let cipher = ChaCha20Poly1305::new(key.into());
    let ciphertext = cipher
        .encrypt(
            Nonce::from_slice(&nonce),
            Payload {
                msg: &plaintext,
                aad: BUNDLE_AAD,
            },
        )
        .map_err(|_| BundleError::AuthenticationFailed)?;
    Ok(SealedBundle { nonce, ciphertext })
}

pub(super) fn open_bundle(
    key: &[u8; 32],
    sealed: &SealedBundle,
    carriers: &[BundleCarrier<'_>],
) -> Result<Vec<Vec<u8>>, BundleError> {
    let ciphertext_limit = MAX_PLAINTEXT_SIZE + AEAD_TAG_SIZE;
    if sealed.ciphertext.len() > ciphertext_limit {
        return Err(BundleError::BudgetExceeded {
            field: "密文总长度",
            limit: ciphertext_limit,
            actual: sealed.ciphertext.len(),
        });
    }
    let cipher = ChaCha20Poly1305::new(key.into());
    let plaintext = cipher
        .decrypt(
            Nonce::from_slice(&sealed.nonce),
            Payload {
                msg: &sealed.ciphertext,
                aad: BUNDLE_AAD,
            },
        )
        .map_err(|_| BundleError::AuthenticationFailed)?;
    let decoded = decode_bundle(&plaintext)?;
    if decoded.entries.len() != carriers.len() {
        return Err(BundleError::CarrierCountMismatch {
            expected: decoded.entries.len(),
            actual: carriers.len(),
        });
    }

    let mut extractions = Vec::with_capacity(decoded.entries.len());
    let mut restored = Vec::with_capacity(decoded.entries.len());
    for (index, (entry, carrier)) in decoded.entries.into_iter().zip(carriers).enumerate() {
        if entry.identity != carrier.identity {
            return Err(BundleError::CarrierIdentityMismatch {
                index,
                expected: entry.identity,
                actual: carrier.identity.to_owned(),
            });
        }
        let mut payload = entry.payload;
        bind_stubs_from_carrier(carrier.carrier, &mut payload)?;
        let extraction = Extraction {
            carrier: carrier.carrier.to_vec(),
            payload,
        };
        restored.push(restore(&extraction.carrier, &extraction.payload)?);
        extractions.push(extraction);
    }

    let members = carriers
        .iter()
        .zip(&restored)
        .zip(&extractions)
        .map(|((carrier, original), extraction)| {
            GroupMember::new(carrier.identity, original, extraction)
        })
        .collect::<Vec<_>>();
    if encode_manifest(&members)? != decoded.manifest {
        return Err(BundleError::ManifestMismatch);
    }
    Ok(restored)
}

fn encode_bundle(manifest: &[u8], members: &[GroupMember<'_>]) -> Result<Vec<u8>, BundleError> {
    ensure_count("DEX 数量", members.len(), MAX_MEMBER_COUNT)?;
    let encoded_size = encoded_bundle_size(manifest, members)?;
    let mut encoded = Vec::with_capacity(encoded_size);
    encoded.extend_from_slice(BUNDLE_MAGIC);
    encoded.extend_from_slice(&BUNDLE_VERSION.to_le_bytes());
    push_bytes_u32(&mut encoded, "清单", manifest)?;
    push_u32(&mut encoded, "DEX 数量", members.len())?;
    for member in members {
        ensure_count("DEX 身份", member.identity.len(), MAX_IDENTITY_SIZE)?;
        push_bytes_u16(&mut encoded, "DEX 身份", member.identity.as_bytes())?;
        encoded.extend_from_slice(
            &(member.extraction.payload.original_file_size as u64).to_le_bytes(),
        );
        encoded.extend_from_slice(&member.extraction.payload.carrier_sha256);
        ensure_count(
            "方法数量",
            member.extraction.payload.methods.len(),
            MAX_METHOD_COUNT,
        )?;
        push_u32(
            &mut encoded,
            "方法数量",
            member.extraction.payload.methods.len(),
        )?;
        for method in &member.extraction.payload.methods {
            ensure_count("原始方法指令", method.original.len(), MAX_METHOD_CODE_SIZE)?;
            encoded.extend_from_slice(&method.method_index.to_le_bytes());
            encoded.extend_from_slice(&method.instructions_offset.to_le_bytes());
            push_bytes_u32(&mut encoded, "原始方法指令", &method.original)?;
        }
    }
    Ok(encoded)
}

fn encoded_bundle_size(manifest: &[u8], members: &[GroupMember<'_>]) -> Result<usize, BundleError> {
    let mut size = 4usize + 4 + 4 + 4;
    size = checked_size_add(size, manifest.len())?;
    for member in members {
        size = checked_size_add(size, 2 + member.identity.len() + 8 + 32 + 4)?;
        for method in &member.extraction.payload.methods {
            size = checked_size_add(size, 4 + 4 + 4)?;
            size = checked_size_add(size, method.original.len())?;
        }
    }
    Ok(size)
}

fn checked_size_add(current: usize, added: usize) -> Result<usize, BundleError> {
    let actual = current.saturating_add(added);
    if actual > MAX_PLAINTEXT_SIZE {
        return Err(BundleError::BudgetExceeded {
            field: "明文总长度",
            limit: MAX_PLAINTEXT_SIZE,
            actual,
        });
    }
    Ok(actual)
}

struct DecodedBundle {
    manifest: Vec<u8>,
    entries: Vec<DecodedEntry>,
}

struct DecodedEntry {
    identity: String,
    payload: CodePayload,
}

fn decode_bundle(data: &[u8]) -> Result<DecodedBundle, BundleError> {
    if data.len() > MAX_PLAINTEXT_SIZE {
        return Err(BundleError::BudgetExceeded {
            field: "明文总长度",
            limit: MAX_PLAINTEXT_SIZE,
            actual: data.len(),
        });
    }
    let mut reader = Reader::new(data);
    if reader.read_exact(4, "magic")? != BUNDLE_MAGIC {
        return Err(BundleError::InvalidMagic);
    }
    let version = reader.read_u32("版本")?;
    if version != BUNDLE_VERSION {
        return Err(BundleError::UnsupportedVersion { actual: version });
    }
    let manifest = reader.read_vec_u32("清单", MAX_PLAINTEXT_SIZE)?;
    let member_count = reader.read_u32("DEX 数量")? as usize;
    ensure_count("DEX 数量", member_count, MAX_MEMBER_COUNT)?;
    let mut identities = HashSet::with_capacity(member_count);
    let mut entries = Vec::with_capacity(member_count);
    for _ in 0..member_count {
        let identity_bytes = reader.read_vec_u16("DEX 身份", MAX_IDENTITY_SIZE)?;
        let identity =
            String::from_utf8(identity_bytes).map_err(|_| BundleError::InvalidIdentity)?;
        if !identities.insert(identity.clone()) {
            return Err(BundleError::DuplicateIdentity { identity });
        }
        let original_file_size_u64 = reader.read_u64("原始 DEX 长度")?;
        let original_file_size =
            usize::try_from(original_file_size_u64).map_err(|_| BundleError::ValueTooLarge {
                field: "原始 DEX 长度",
                actual: usize::MAX,
            })?;
        let carrier_sha256 = reader.read_array_32("载体摘要")?;
        let method_count = reader.read_u32("方法数量")? as usize;
        ensure_count("方法数量", method_count, MAX_METHOD_COUNT)?;
        let mut methods = Vec::with_capacity(method_count);
        for _ in 0..method_count {
            let method_index = reader.read_u32("方法索引")?;
            let instructions_offset = reader.read_u32("指令偏移")?;
            let original = reader.read_vec_u32("原始方法指令", MAX_METHOD_CODE_SIZE)?;
            methods.push(ExtractedMethod {
                method_index,
                instructions_offset,
                original,
                stub: Vec::new(),
            });
        }
        entries.push(DecodedEntry {
            identity,
            payload: CodePayload {
                carrier_sha256,
                original_file_size,
                methods,
            },
        });
    }
    if reader.remaining() != 0 {
        return Err(BundleError::TrailingData {
            remaining: reader.remaining(),
        });
    }
    Ok(DecodedBundle { manifest, entries })
}

fn ensure_count(field: &'static str, count: usize, limit: usize) -> Result<(), BundleError> {
    if count > limit {
        Err(BundleError::BudgetExceeded {
            field,
            limit,
            actual: count,
        })
    } else {
        Ok(())
    }
}

fn push_u32(output: &mut Vec<u8>, field: &'static str, value: usize) -> Result<(), BundleError> {
    let value = u32::try_from(value).map_err(|_| BundleError::ValueTooLarge {
        field,
        actual: value,
    })?;
    output.extend_from_slice(&value.to_le_bytes());
    Ok(())
}

fn push_bytes_u32(
    output: &mut Vec<u8>,
    field: &'static str,
    value: &[u8],
) -> Result<(), BundleError> {
    push_u32(output, field, value.len())?;
    output.extend_from_slice(value);
    Ok(())
}

fn push_bytes_u16(
    output: &mut Vec<u8>,
    field: &'static str,
    value: &[u8],
) -> Result<(), BundleError> {
    let length = u16::try_from(value.len()).map_err(|_| BundleError::ValueTooLarge {
        field,
        actual: value.len(),
    })?;
    output.extend_from_slice(&length.to_le_bytes());
    output.extend_from_slice(value);
    Ok(())
}

struct Reader<'a> {
    data: &'a [u8],
    cursor: usize,
}

impl<'a> Reader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, cursor: 0 }
    }

    fn remaining(&self) -> usize {
        self.data.len().saturating_sub(self.cursor)
    }

    fn read_exact(&mut self, length: usize, field: &'static str) -> Result<&'a [u8], BundleError> {
        if length > self.remaining() {
            return Err(BundleError::Truncated {
                field,
                needed: length,
                remaining: self.remaining(),
            });
        }
        let end = self.cursor + length;
        let value = &self.data[self.cursor..end];
        self.cursor = end;
        Ok(value)
    }

    fn read_u16(&mut self, field: &'static str) -> Result<u16, BundleError> {
        Ok(u16::from_le_bytes(
            self.read_exact(2, field)?.try_into().unwrap(),
        ))
    }

    fn read_u32(&mut self, field: &'static str) -> Result<u32, BundleError> {
        Ok(u32::from_le_bytes(
            self.read_exact(4, field)?.try_into().unwrap(),
        ))
    }

    fn read_u64(&mut self, field: &'static str) -> Result<u64, BundleError> {
        Ok(u64::from_le_bytes(
            self.read_exact(8, field)?.try_into().unwrap(),
        ))
    }

    fn read_array_32(&mut self, field: &'static str) -> Result<[u8; 32], BundleError> {
        Ok(self.read_exact(32, field)?.try_into().unwrap())
    }

    fn read_vec_u16(&mut self, field: &'static str, limit: usize) -> Result<Vec<u8>, BundleError> {
        let length = self.read_u16(field)? as usize;
        self.read_vec(length, field, limit)
    }

    fn read_vec_u32(&mut self, field: &'static str, limit: usize) -> Result<Vec<u8>, BundleError> {
        let length = self.read_u32(field)? as usize;
        self.read_vec(length, field, limit)
    }

    fn read_vec(
        &mut self,
        length: usize,
        field: &'static str,
        limit: usize,
    ) -> Result<Vec<u8>, BundleError> {
        if length > limit {
            return Err(BundleError::BudgetExceeded {
                field,
                limit,
                actual: length,
            });
        }
        Ok(self.read_exact(length, field)?.to_vec())
    }
}

#[cfg(test)]
mod tests {
    use super::{
        checked_size_add, decode_bundle, BundleError, BUNDLE_MAGIC, BUNDLE_VERSION,
        MAX_MEMBER_COUNT, MAX_PLAINTEXT_SIZE,
    };

    #[test]
    fn 解析拒绝截断版本与尾随数据() {
        assert!(matches!(
            decode_bundle(&BUNDLE_MAGIC[..3]),
            Err(BundleError::Truncated { .. })
        ));

        let mut unsupported = Vec::from(BUNDLE_MAGIC.as_slice());
        unsupported.extend_from_slice(&(BUNDLE_VERSION + 1).to_le_bytes());
        assert!(matches!(
            decode_bundle(&unsupported),
            Err(BundleError::UnsupportedVersion { actual }) if actual == BUNDLE_VERSION + 1
        ));

        let mut trailing = Vec::from(BUNDLE_MAGIC.as_slice());
        trailing.extend_from_slice(&BUNDLE_VERSION.to_le_bytes());
        trailing.extend_from_slice(&0u32.to_le_bytes());
        trailing.extend_from_slice(&0u32.to_le_bytes());
        trailing.push(1);
        assert!(matches!(
            decode_bundle(&trailing),
            Err(BundleError::TrailingData { remaining: 1 })
        ));
    }

    #[test]
    fn 解析在分配前拒绝超长字段() {
        let mut oversized = Vec::from(BUNDLE_MAGIC.as_slice());
        oversized.extend_from_slice(&BUNDLE_VERSION.to_le_bytes());
        oversized.extend_from_slice(&u32::MAX.to_le_bytes());
        assert!(matches!(
            decode_bundle(&oversized),
            Err(BundleError::BudgetExceeded { .. })
        ));

        let mut too_many_members = Vec::from(BUNDLE_MAGIC.as_slice());
        too_many_members.extend_from_slice(&BUNDLE_VERSION.to_le_bytes());
        too_many_members.extend_from_slice(&0u32.to_le_bytes());
        too_many_members.extend_from_slice(&((MAX_MEMBER_COUNT + 1) as u32).to_le_bytes());
        assert!(matches!(
            decode_bundle(&too_many_members),
            Err(BundleError::BudgetExceeded {
                field: "DEX 数量",
                ..
            })
        ));

        assert!(matches!(
            checked_size_add(MAX_PLAINTEXT_SIZE, 1),
            Err(BundleError::BudgetExceeded {
                field: "明文总长度",
                ..
            })
        ));
    }
}

//! 所选记录的绑定及材料快照；不哈希整个数据库，不输出路径、Alias 或密码。
use super::{
    decrypt_record_secrets, get_default_certificate_id, read_encrypted_certificate,
    CertificateRecord, CertificateStoreState,
};
use crate::certificates::CertificateSelection;
use rusqlite::OptionalExtension;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CertificateBindingError {
    StoreUnavailable,
    NotFound,
    RecordChanged,
    CredentialsInvalid,
    UnsupportedVersion,
}
impl std::fmt::Display for CertificateBindingError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::StoreUnavailable => "无法读取所选证书的绑定状态",
            Self::NotFound => "所选证书或默认项不存在，不会改用其他证书",
            Self::RecordChanged => "所选证书材料或签名偏好已变化，请重新规划",
            Self::CredentialsInvalid => "所选证书的本机密文无法解密，不会回退其他来源",
            Self::UnsupportedVersion => "证书绑定版本不受支持，请重新规划",
        })
    }
}
impl std::error::Error for CertificateBindingError {}

/// 非秘密的记录承诺；不是授权凭证，也不代表 keystore 文件字节已经固定。
/// 仅序列化版本、编号及不含秘密的承诺；接收绑定不等于获得操作授权。
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CertificateBinding {
    schema_version: u32,
    id: String,
    state_sha256: [u8; 32],
}
impl CertificateBinding {
    pub fn schema_version(&self) -> u32 {
        self.schema_version
    }
    pub fn certificate_id(&self) -> &str {
        &self.id
    }
    pub fn state_sha256(&self) -> [u8; 32] {
        self.state_sha256
    }
}

/// 内部材料与记录承诺来自同一个 SQLite 读快照；不能通用序列化或调试。
///
/// ```compile_fail
/// use shield_services::certificates::BoundSigningIdentity;
/// fn 输出(value: &BoundSigningIdentity) { serde_json::to_string(value).unwrap(); }
/// ```
/// ```compile_fail
/// use shield_services::certificates::BoundSigningIdentity;
/// fn 输出(value: &BoundSigningIdentity) { format!("{value:?}"); }
/// ```
pub struct BoundSigningIdentity {
    certificate: CertificateRecord,
    binding: CertificateBinding,
}

/// 绑定只读取这组字段，避免无关元数据的类型或大小影响提交前复核。
struct BindingMaterial {
    // 固定顺序：编号、路径、Alias、类型、Keystore 密文、Key 密文。
    fields: [String; 6],
    versions: [u8; 4],
}
impl From<&CertificateRecord> for BindingMaterial {
    fn from(record: &CertificateRecord) -> Self {
        Self {
            fields: [
                record.id.clone(),
                record.keystore_path.clone(),
                record.key_alias.clone(),
                record.ks_type.clone(),
                record.keystore_password.clone(),
                record.key_password.clone(),
            ],
            versions: [
                u8::from(record.sign_v1),
                u8::from(record.sign_v2),
                u8::from(record.sign_v3),
                u8::from(record.sign_v4),
            ],
        }
    }
}
impl BoundSigningIdentity {
    pub fn binding(&self) -> &CertificateBinding {
        &self.binding
    }
    pub fn certificate(&self) -> &CertificateRecord {
        &self.certificate
    }
    pub fn into_parts(self) -> (CertificateRecord, CertificateBinding) {
        (self.certificate, self.binding)
    }
}

impl CertificateStoreState {
    pub fn capture_signing_identity(
        &self,
        selection: CertificateSelection<'_>,
    ) -> Result<BoundSigningIdentity, CertificateBindingError> {
        let connection = self
            .open()
            .map_err(|_| CertificateBindingError::StoreUnavailable)?;
        let transaction = connection
            .unchecked_transaction()
            .map_err(|_| CertificateBindingError::StoreUnavailable)?;
        // 首次 SELECT 固定读快照；默认项和选中记录不能分别打开连接拼接。
        let default = get_default_certificate_id(&transaction)
            .map_err(|_| CertificateBindingError::StoreUnavailable)?;
        let id = match selection {
            CertificateSelection::Id(id) if !id.trim().is_empty() => id,
            CertificateSelection::Id(_) => return Err(CertificateBindingError::NotFound),
            CertificateSelection::Default => default
                .as_deref()
                .ok_or(CertificateBindingError::NotFound)?,
        };
        let mut certificate = read_encrypted_certificate(&transaction, id)
            .map_err(|_| CertificateBindingError::StoreUnavailable)?
            .ok_or(CertificateBindingError::NotFound)?;
        let binding = self.bind_encrypted_record(&BindingMaterial::from(&certificate));
        decrypt_record_secrets(&self.crypto_key, &mut certificate)
            .map_err(|_| CertificateBindingError::CredentialsInvalid)?;
        certificate.is_default = default.as_deref() == Some(id);
        if certificate.key_password.is_empty() {
            certificate.key_password = certificate.keystore_password.clone();
        }
        Ok(BoundSigningIdentity {
            certificate,
            binding,
        })
    }

    /// 只按绑定编号核对原记录，不查询新的默认项，也不解密密码或启动 Java。
    pub fn check_certificate_binding(
        &self,
        expected: &CertificateBinding,
    ) -> Result<(), CertificateBindingError> {
        if expected.schema_version != 1 {
            return Err(CertificateBindingError::UnsupportedVersion);
        }
        let connection = self
            .open()
            .map_err(|_| CertificateBindingError::StoreUnavailable)?;
        let record = connection.query_row(
            "SELECT id,keystore_path,key_alias,ks_type,keystore_password,key_password,sign_v1,sign_v2,sign_v3,sign_v4
             FROM certificates WHERE id=?1",
            [&expected.id], |row| Ok(BindingMaterial {
                fields: [row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?],
                versions: [u8::from(row.get::<_,i64>(6)? != 0), u8::from(row.get::<_,i64>(7)? != 0),
                    u8::from(row.get::<_,i64>(8)? != 0), u8::from(row.get::<_,i64>(9)? != 0)],
            }),
        ).optional().map_err(|_| CertificateBindingError::StoreUnavailable)?.ok_or(CertificateBindingError::NotFound)?;
        if self.bind_encrypted_record(&record) != *expected {
            return Err(CertificateBindingError::RecordChanged);
        }
        Ok(())
    }

    fn bind_encrypted_record(&self, record: &BindingMaterial) -> CertificateBinding {
        let mut digest = Sha256::new();
        // 绑定上下文使用原路径身份，不 canonicalize；不把明文密码当作公开可猜测的摘要输入。
        for field in [
            "mocika-shared-certificate-binding-v1",
            self.db_path.to_string_lossy().as_ref(),
            self.keystore_dir.to_string_lossy().as_ref(),
            &record.fields[0],
            &record.fields[1],
            &record.fields[2],
            &record.fields[3],
            &record.fields[4],
            &record.fields[5],
        ] {
            digest.update((field.len() as u64).to_le_bytes());
            digest.update(field.as_bytes());
        }
        digest.update(record.versions);
        // 名称、备注、校验状态／时间、默认项及自动签名偏好不改变已经明确的签名任务。
        CertificateBinding {
            schema_version: 1,
            id: record.fields[0].clone(),
            state_sha256: digest.finalize().into(),
        }
    }
}

#[cfg(test)]
mod tests;

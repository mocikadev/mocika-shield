//! 证书输入、无秘密摘要及仅供执行使用的材料类型。
use serde::{Deserialize, Serialize};

pub enum CertificateSelection<'a> {
    Id(&'a str),
    Default,
}

/// 只允许修改显示与签名偏好；未指定字段在事务内保留，不携带证书材料。
#[derive(Clone, Default)]
pub struct CertificatePreferencesPatch {
    pub name: Option<String>,
    pub note: Option<String>,
    pub sign_v1: Option<bool>,
    pub sign_v2: Option<bool>,
    pub sign_v3: Option<bool>,
    pub sign_v4: Option<bool>,
    pub auto_sign_enabled: Option<bool>,
}

/// 仅供本机执行使用的签名材料；不提供序列化或调试输出。
///
/// ```compile_fail
/// use shield_app::certificates::CertificateRecord;
/// fn 输出材料(value: &CertificateRecord) -> String { format!("{value:?}") }
/// ```
/// ```compile_fail
/// use shield_app::certificates::CertificateRecord;
/// fn 输出材料(value: &CertificateRecord) { serde_json::to_string(value).unwrap(); }
/// ```
#[derive(Clone)]
pub struct CertificateRecord {
    pub id: String,
    pub name: String,
    pub source_type: String,
    pub keystore_path: String,
    pub keystore_password: String,
    pub key_alias: String,
    pub key_password: String,
    pub ks_type: String,
    pub sign_v1: bool,
    pub sign_v2: bool,
    pub sign_v3: bool,
    pub sign_v4: bool,
    pub auto_sign_enabled: bool,
    pub note: String,
    pub is_default: bool,
    pub created_at: i64,
    pub updated_at: i64,
    pub last_verified_at: Option<i64>,
    pub last_verify_status: String,
    pub last_verify_message: Option<String>,
}

#[derive(Clone, Deserialize)]
pub struct CertificateUpsertInput {
    pub id: Option<String>,
    pub name: String,
    pub source_type: String,
    pub keystore_path: String,
    pub keystore_password: String,
    pub key_alias: String,
    pub key_password: String,
    pub ks_type: Option<String>,
    pub sign_v1: bool,
    pub sign_v2: bool,
    pub sign_v3: bool,
    pub sign_v4: bool,
    pub auto_sign_enabled: bool,
    pub note: String,
    pub set_as_default: bool,
    pub copy_keystore_to_managed: bool,
    pub managed_file_name: Option<String>,
}

#[derive(Clone, Deserialize)]
pub struct CertificateValidationInput {
    pub keystore_path: String,
    pub keystore_password: String,
    pub key_alias: String,
    pub ks_type: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CertificateValidationResult {
    pub valid: bool,
    pub aliases: Vec<String>,
    pub resolved_alias: Option<String>,
    pub message: Option<String>,
}

#[derive(Clone, Deserialize)]
pub struct CreateManagedCertificateInput {
    pub name: String,
    pub file_name: String,
    pub key_alias: String,
    pub keystore_password: String,
    pub key_password: String,
    pub ks_type: Option<String>,
    pub sign_v1: bool,
    pub sign_v2: bool,
    pub sign_v3: bool,
    pub sign_v4: bool,
    pub auto_sign_enabled: bool,
    pub note: String,
    pub set_as_default: bool,
    pub dname: String,
    pub validity_days: u32,
    pub key_size: u32,
}

/// 查询和界面返回的证书摘要，结构中不含密码字段。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CertificateSummary {
    pub id: String,
    pub name: String,
    pub source_type: String,
    pub keystore_path: String,
    pub key_alias: String,
    pub ks_type: String,
    pub sign_v1: bool,
    pub sign_v2: bool,
    pub sign_v3: bool,
    pub sign_v4: bool,
    pub auto_sign_enabled: bool,
    pub note: String,
    pub is_default: bool,
    pub created_at: i64,
    pub updated_at: i64,
    pub last_verified_at: Option<i64>,
    pub last_verify_status: String,
    pub last_verify_message: Option<String>,
}
impl From<CertificateRecord> for CertificateSummary {
    fn from(value: CertificateRecord) -> Self {
        Self {
            id: value.id,
            name: value.name,
            source_type: value.source_type,
            keystore_path: value.keystore_path,
            key_alias: value.key_alias,
            ks_type: value.ks_type,
            sign_v1: value.sign_v1,
            sign_v2: value.sign_v2,
            sign_v3: value.sign_v3,
            sign_v4: value.sign_v4,
            auto_sign_enabled: value.auto_sign_enabled,
            note: value.note,
            is_default: value.is_default,
            created_at: value.created_at,
            updated_at: value.updated_at,
            last_verified_at: value.last_verified_at,
            last_verify_status: value.last_verify_status,
            last_verify_message: value.last_verify_message,
        }
    }
}

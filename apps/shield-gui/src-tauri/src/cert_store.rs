use crate::app_config::normalize_keystore_type;
use crate::app_paths::strip_unc_prefix;
use chacha20poly1305::{
    aead::{Aead, KeyInit},
    ChaCha20Poly1305, Nonce,
};
use rand::RngCore;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::Manager;
use uuid::Uuid;

const DB_FILE: &str = "shield.db";
const APP_STATE_DEFAULT_CERTIFICATE_ID: &str = "default_certificate_id";
const SECRET_PREFIX: &str = "enc:v1:";
type VerifyState = (String, Option<String>, Option<i64>);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct CertificateRecord {
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct CertificateUpsertInput {
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct CertificateValidationInput {
    pub keystore_path: String,
    pub keystore_password: String,
    pub key_alias: String,
    pub ks_type: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct CertificateValidationResult {
    pub valid: bool,
    pub aliases: Vec<String>,
    pub resolved_alias: Option<String>,
    pub message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct CreateManagedCertificateInput {
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

pub(crate) struct CertificateStoreState {
    db_path: PathBuf,
    keystore_dir: PathBuf,
    crypto_key: [u8; 32],
}

impl CertificateStoreState {
    pub(crate) fn new(db_path: PathBuf, keystore_dir: PathBuf) -> Self {
        let crypto_key = derive_crypto_key(&db_path, &keystore_dir);
        Self {
            db_path,
            keystore_dir,
            crypto_key,
        }
    }

    pub(crate) fn keystore_dir(&self) -> &Path {
        &self.keystore_dir
    }

    pub(crate) fn open(&self) -> Result<Connection, String> {
        open_connection(&self.db_path)
    }

    pub(crate) fn list_certificates(&self) -> Result<Vec<CertificateRecord>, String> {
        let conn = self.open()?;
        let default_id = get_default_certificate_id(&conn)?;
        let mut stmt = conn
            .prepare(
                "SELECT id, name, source_type, keystore_path, keystore_password, key_alias, \
                 key_password, ks_type, sign_v1, sign_v2, sign_v3, sign_v4, auto_sign_enabled, \
                 note, created_at, updated_at, last_verified_at, last_verify_status, last_verify_message \
                 FROM certificates ORDER BY updated_at DESC, created_at DESC",
            )
            .map_err(|e| format!("准备证书查询失败: {e}"))?;
        let rows = stmt
            .query_map([], |row| {
                Ok(CertificateRecord {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    source_type: row.get(2)?,
                    keystore_path: row.get(3)?,
                    keystore_password: row.get(4)?,
                    key_alias: row.get(5)?,
                    key_password: row.get(6)?,
                    ks_type: row.get(7)?,
                    sign_v1: row.get::<_, i64>(8)? != 0,
                    sign_v2: row.get::<_, i64>(9)? != 0,
                    sign_v3: row.get::<_, i64>(10)? != 0,
                    sign_v4: row.get::<_, i64>(11)? != 0,
                    auto_sign_enabled: row.get::<_, i64>(12)? != 0,
                    note: row.get(13)?,
                    is_default: false,
                    created_at: row.get(14)?,
                    updated_at: row.get(15)?,
                    last_verified_at: row.get(16)?,
                    last_verify_status: row.get(17)?,
                    last_verify_message: row.get(18)?,
                })
            })
            .map_err(|e| format!("读取证书列表失败: {e}"))?;

        let mut items = Vec::new();
        for row in rows {
            let mut item = row.map_err(|e| format!("解析证书记录失败: {e}"))?;
            decrypt_record_secrets(&self.crypto_key, &mut item)?;
            item.is_default = default_id
                .as_deref()
                .is_some_and(|default_id| default_id == item.id);
            items.push(item);
        }
        Ok(items)
    }

    pub(crate) fn get_certificate(&self, id: &str) -> Result<Option<CertificateRecord>, String> {
        let conn = self.open()?;
        let default_id = get_default_certificate_id(&conn)?;
        let mut stmt = conn
            .prepare(
                "SELECT id, name, source_type, keystore_path, keystore_password, key_alias, \
                 key_password, ks_type, sign_v1, sign_v2, sign_v3, sign_v4, auto_sign_enabled, \
                 note, created_at, updated_at, last_verified_at, last_verify_status, last_verify_message \
                 FROM certificates WHERE id = ?1",
            )
            .map_err(|e| format!("准备证书详情查询失败: {e}"))?;
        let result = stmt
            .query_row([id], |row| {
                Ok(CertificateRecord {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    source_type: row.get(2)?,
                    keystore_path: row.get(3)?,
                    keystore_password: row.get(4)?,
                    key_alias: row.get(5)?,
                    key_password: row.get(6)?,
                    ks_type: row.get(7)?,
                    sign_v1: row.get::<_, i64>(8)? != 0,
                    sign_v2: row.get::<_, i64>(9)? != 0,
                    sign_v3: row.get::<_, i64>(10)? != 0,
                    sign_v4: row.get::<_, i64>(11)? != 0,
                    auto_sign_enabled: row.get::<_, i64>(12)? != 0,
                    note: row.get(13)?,
                    is_default: false,
                    created_at: row.get(14)?,
                    updated_at: row.get(15)?,
                    last_verified_at: row.get(16)?,
                    last_verify_status: row.get(17)?,
                    last_verify_message: row.get(18)?,
                })
            })
            .optional()
            .map_err(|e| format!("读取证书详情失败: {e}"))?;

        let Some(mut item) = result else {
            return Ok(None);
        };
        decrypt_record_secrets(&self.crypto_key, &mut item)?;
        Ok(Some({
            item.is_default = default_id
                .as_deref()
                .is_some_and(|default_id| default_id == item.id);
            item
        }))
    }

    pub(crate) fn save_certificate(
        &self,
        input: &CertificateUpsertInput,
        keystore_path: &str,
        resolved_alias: &str,
        verify_status: Option<(&str, Option<&str>, i64)>,
    ) -> Result<CertificateRecord, String> {
        let mut conn = self.open()?;
        let tx = conn
            .transaction()
            .map_err(|e| format!("创建证书事务失败: {e}"))?;
        let now = now_timestamp();
        let source_type = normalize_source_type(&input.source_type).to_string();
        let ks_type =
            normalize_keystore_type(input.ks_type.as_deref()).unwrap_or_else(|| "JKS".to_string());
        let id = input
            .id
            .clone()
            .unwrap_or_else(|| Uuid::new_v4().to_string());
        let created_at = select_created_at(&tx, &id)?.unwrap_or(now);
        let encrypted_keystore_password =
            encrypt_secret(&self.crypto_key, &input.keystore_password)?;
        let encrypted_key_password = encrypt_secret(&self.crypto_key, &input.key_password)?;

        let (last_verified_status, last_verified_message, last_verified_at) =
            if let Some((status, message, verified_at)) = verify_status {
                (
                    status.to_string(),
                    message.map(|value| value.to_string()),
                    Some(verified_at),
                )
            } else if let Some(existing) = select_verify_state(&tx, &id)? {
                existing
            } else {
                ("unknown".to_string(), None, None)
            };

        tx.execute(
            "INSERT INTO certificates (
                id, name, source_type, keystore_path, keystore_password, key_alias, key_password,
                ks_type, sign_v1, sign_v2, sign_v3, sign_v4, auto_sign_enabled, note,
                created_at, updated_at, last_verified_at, last_verify_status, last_verify_message
             ) VALUES (
                ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19
             )
             ON CONFLICT(id) DO UPDATE SET
                name = excluded.name,
                source_type = excluded.source_type,
                keystore_path = excluded.keystore_path,
                keystore_password = excluded.keystore_password,
                key_alias = excluded.key_alias,
                key_password = excluded.key_password,
                ks_type = excluded.ks_type,
                sign_v1 = excluded.sign_v1,
                sign_v2 = excluded.sign_v2,
                sign_v3 = excluded.sign_v3,
                sign_v4 = excluded.sign_v4,
                auto_sign_enabled = excluded.auto_sign_enabled,
                note = excluded.note,
                updated_at = excluded.updated_at,
                last_verified_at = excluded.last_verified_at,
                last_verify_status = excluded.last_verify_status,
                last_verify_message = excluded.last_verify_message",
            params![
                id,
                normalize_name(&input.name, resolved_alias),
                source_type,
                keystore_path,
                encrypted_keystore_password,
                resolved_alias,
                encrypted_key_password,
                ks_type,
                bool_to_int(input.sign_v1),
                bool_to_int(input.sign_v2),
                bool_to_int(input.sign_v3),
                bool_to_int(input.sign_v4),
                bool_to_int(input.auto_sign_enabled),
                input.note.trim(),
                created_at,
                now,
                last_verified_at,
                last_verified_status,
                last_verified_message,
            ],
        )
        .map_err(|e| format!("保存证书失败: {e}"))?;

        if input.set_as_default {
            set_default_certificate_id_tx(&tx, Some(&id))?;
        }

        tx.commit().map_err(|e| format!("提交证书保存失败: {e}"))?;
        self.get_certificate(&id)?
            .ok_or_else(|| "保存证书后未能重新读取记录".to_string())
    }

    pub(crate) fn set_default_certificate(&self, id: Option<&str>) -> Result<(), String> {
        let mut conn = self.open()?;
        let tx = conn
            .transaction()
            .map_err(|e| format!("创建默认证书事务失败: {e}"))?;
        if let Some(id) = id {
            let exists = tx
                .query_row("SELECT 1 FROM certificates WHERE id = ?1", [id], |_| Ok(()))
                .optional()
                .map_err(|e| format!("查询默认证书失败: {e}"))?
                .is_some();
            if !exists {
                return Err("要设置的默认证书不存在".to_string());
            }
        }
        set_default_certificate_id_tx(&tx, id)?;
        tx.commit().map_err(|e| format!("更新默认证书失败: {e}"))
    }

    pub(crate) fn update_certificate_preferences(
        &self,
        input: &CertificateUpsertInput,
    ) -> Result<CertificateRecord, String> {
        let id = input
            .id
            .as_deref()
            .ok_or_else(|| "缺少要更新的证书 ID".to_string())?;
        let existing = self
            .get_certificate(id)?
            .ok_or_else(|| "要更新的证书不存在".to_string())?;
        let mut conn = self.open()?;
        let tx = conn
            .transaction()
            .map_err(|e| format!("创建证书更新事务失败: {e}"))?;
        let now = now_timestamp();

        tx.execute(
            "UPDATE certificates
             SET name = ?2,
                 sign_v1 = ?3,
                 sign_v2 = ?4,
                 sign_v3 = ?5,
                 sign_v4 = ?6,
                 auto_sign_enabled = ?7,
                 note = ?8,
                 updated_at = ?9
             WHERE id = ?1",
            params![
                id,
                normalize_name(&input.name, &existing.key_alias),
                bool_to_int(input.sign_v1),
                bool_to_int(input.sign_v2),
                bool_to_int(input.sign_v3),
                bool_to_int(input.sign_v4),
                bool_to_int(input.auto_sign_enabled),
                input.note.trim(),
                now,
            ],
        )
        .map_err(|e| format!("更新证书偏好失败: {e}"))?;

        if input.set_as_default {
            set_default_certificate_id_tx(&tx, Some(id))?;
        }

        tx.commit().map_err(|e| format!("提交证书更新失败: {e}"))?;
        self.get_certificate(id)?
            .ok_or_else(|| "更新证书后未能重新读取记录".to_string())
    }

    pub(crate) fn delete_certificate(
        &self,
        id: &str,
        remove_keystore_file: bool,
    ) -> Result<(), String> {
        let record = self
            .get_certificate(id)?
            .ok_or_else(|| "要删除的证书不存在".to_string())?;
        let mut conn = self.open()?;
        let tx = conn
            .transaction()
            .map_err(|e| format!("创建删除证书事务失败: {e}"))?;
        tx.execute("DELETE FROM certificates WHERE id = ?1", [id])
            .map_err(|e| format!("删除证书记录失败: {e}"))?;
        let default_id = get_default_certificate_id(&tx)?;
        if default_id.as_deref().is_some_and(|value| value == id) {
            set_default_certificate_id_tx(&tx, None)?;
        }
        tx.commit().map_err(|e| format!("提交证书删除失败: {e}"))?;

        if remove_keystore_file && record.source_type == "managed" {
            let path = PathBuf::from(&record.keystore_path);
            if path.exists() {
                fs::remove_file(&path).map_err(|e| format!("删除托管 keystore 文件失败: {e}"))?;
            }
        }
        Ok(())
    }

    pub(crate) fn update_verify_status(
        &self,
        id: &str,
        status: &str,
        message: Option<&str>,
        resolved_alias: Option<&str>,
    ) -> Result<CertificateRecord, String> {
        let conn = self.open()?;
        let now = now_timestamp();
        conn.execute(
            "UPDATE certificates
             SET last_verified_at = ?2,
                 last_verify_status = ?3,
                 last_verify_message = ?4,
                 key_alias = COALESCE(?5, key_alias),
                 updated_at = ?2
             WHERE id = ?1",
            params![id, now, status, message, resolved_alias],
        )
        .map_err(|e| format!("更新证书校验状态失败: {e}"))?;
        self.get_certificate(id)?
            .ok_or_else(|| "更新校验状态后未能重新读取证书".to_string())
    }
}

pub(crate) fn initialize_certificate_store(
    app: &tauri::AppHandle,
) -> Result<CertificateStoreState, String> {
    let data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("无法定位应用数据目录: {e}"))?;
    fs::create_dir_all(&data_dir).map_err(|e| format!("创建应用数据目录失败: {e}"))?;
    tighten_dir_permissions(&data_dir)?;
    let db_path = strip_unc_prefix(data_dir.join(DB_FILE));
    let keystore_dir = strip_unc_prefix(data_dir.join("keystores"));
    fs::create_dir_all(&keystore_dir).map_err(|e| format!("创建 keystore 目录失败: {e}"))?;
    tighten_dir_permissions(&keystore_dir)?;
    let state = CertificateStoreState::new(db_path, keystore_dir);
    let conn = state.open()?;
    initialize_schema(&conn)?;
    tighten_file_permissions(&state.db_path)?;
    Ok(state)
}

fn open_connection(path: &Path) -> Result<Connection, String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("创建数据库目录失败: {e}"))?;
    }
    Connection::open(path).map_err(|e| format!("打开证书数据库失败: {e}"))
}

fn initialize_schema(conn: &Connection) -> Result<(), String> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS certificates (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            source_type TEXT NOT NULL,
            keystore_path TEXT NOT NULL,
            keystore_password TEXT NOT NULL,
            key_alias TEXT NOT NULL,
            key_password TEXT NOT NULL,
            ks_type TEXT NOT NULL,
            sign_v1 INTEGER NOT NULL DEFAULT 1,
            sign_v2 INTEGER NOT NULL DEFAULT 1,
            sign_v3 INTEGER NOT NULL DEFAULT 1,
            sign_v4 INTEGER NOT NULL DEFAULT 0,
            auto_sign_enabled INTEGER NOT NULL DEFAULT 1,
            note TEXT NOT NULL DEFAULT '',
            created_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL,
            last_verified_at INTEGER,
            last_verify_status TEXT NOT NULL DEFAULT 'unknown',
            last_verify_message TEXT
        );
        CREATE TABLE IF NOT EXISTS app_state (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL
        );",
    )
    .map_err(|e| format!("初始化证书数据库失败: {e}"))
}

fn get_default_certificate_id(conn: &Connection) -> Result<Option<String>, String> {
    conn.query_row(
        "SELECT value FROM app_state WHERE key = ?1",
        [APP_STATE_DEFAULT_CERTIFICATE_ID],
        |row| row.get(0),
    )
    .optional()
    .map_err(|e| format!("读取默认证书失败: {e}"))
}

fn set_default_certificate_id_tx(
    tx: &rusqlite::Transaction<'_>,
    id: Option<&str>,
) -> Result<(), String> {
    if let Some(id) = id {
        tx.execute(
            "INSERT INTO app_state(key, value) VALUES(?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![APP_STATE_DEFAULT_CERTIFICATE_ID, id],
        )
        .map_err(|e| format!("保存默认证书失败: {e}"))?;
    } else {
        tx.execute(
            "DELETE FROM app_state WHERE key = ?1",
            [APP_STATE_DEFAULT_CERTIFICATE_ID],
        )
        .map_err(|e| format!("清理默认证书失败: {e}"))?;
    }
    Ok(())
}

fn select_created_at(conn: &Connection, id: &str) -> Result<Option<i64>, String> {
    conn.query_row(
        "SELECT created_at FROM certificates WHERE id = ?1",
        [id],
        |row| row.get(0),
    )
    .optional()
    .map_err(|e| format!("读取证书创建时间失败: {e}"))
}

fn select_verify_state(conn: &Connection, id: &str) -> Result<Option<VerifyState>, String> {
    conn.query_row(
        "SELECT last_verify_status, last_verify_message, last_verified_at
         FROM certificates WHERE id = ?1",
        [id],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    )
    .optional()
    .map_err(|e| format!("读取证书校验状态失败: {e}"))
}

fn now_timestamp() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

fn derive_crypto_key(db_path: &Path, keystore_dir: &Path) -> [u8; 32] {
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

fn encrypt_secret(key: &[u8; 32], value: &str) -> Result<String, String> {
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

fn decrypt_record_secrets(key: &[u8; 32], item: &mut CertificateRecord) -> Result<(), String> {
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
    for pair in bytes.chunks_exact(2) {
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

#[cfg(unix)]
fn tighten_dir_permissions(path: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))
        .map_err(|e| format!("收紧目录权限失败: {e}"))
}

#[cfg(not(unix))]
fn tighten_dir_permissions(_path: &Path) -> Result<(), String> {
    Ok(())
}

#[cfg(unix)]
fn tighten_file_permissions(path: &Path) -> Result<(), String> {
    if path.exists() {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))
            .map_err(|e| format!("收紧数据库权限失败: {e}"))?;
    }
    Ok(())
}

#[cfg(not(unix))]
fn tighten_file_permissions(_path: &Path) -> Result<(), String> {
    Ok(())
}

fn bool_to_int(value: bool) -> i64 {
    if value {
        1
    } else {
        0
    }
}

fn normalize_source_type(value: &str) -> &'static str {
    if value == "managed" {
        "managed"
    } else {
        "external"
    }
}

fn normalize_name(value: &str, fallback_alias: &str) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        fallback_alias.trim().to_string()
    } else {
        trimmed.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn create_store() -> (TempDir, CertificateStoreState) {
        let temp = TempDir::new().expect("创建临时目录失败");
        let store = CertificateStoreState::new(
            temp.path().join("shield.db"),
            temp.path().join("keystores"),
        );
        let conn = store.open().expect("打开临时数据库失败");
        initialize_schema(&conn).expect("初始化测试数据库失败");
        (temp, store)
    }

    fn input(id: Option<&str>) -> CertificateUpsertInput {
        CertificateUpsertInput {
            id: id.map(|value| value.to_string()),
            name: "发布证书".to_string(),
            source_type: "managed".to_string(),
            keystore_path: "/old/release.p12".to_string(),
            keystore_password: "old-store-pass".to_string(),
            key_alias: "release".to_string(),
            key_password: "old-key-pass".to_string(),
            ks_type: Some("PKCS12".to_string()),
            sign_v1: true,
            sign_v2: true,
            sign_v3: true,
            sign_v4: false,
            auto_sign_enabled: true,
            note: "初始备注".to_string(),
            set_as_default: false,
            copy_keystore_to_managed: false,
            managed_file_name: None,
        }
    }

    #[test]
    fn 更新证书偏好时不覆盖证书材料字段() {
        let (_temp, store) = create_store();
        let created = store
            .save_certificate(
                &input(None),
                "/old/release.p12",
                "release",
                Some(("success", Some("校验通过"), 100)),
            )
            .expect("保存初始证书失败");

        let mut edit = input(Some(&created.id));
        edit.name = "生产证书".to_string();
        edit.source_type = "external".to_string();
        edit.keystore_path = "/new/changed.jks".to_string();
        edit.keystore_password = "new-store-pass".to_string();
        edit.key_alias = "changed".to_string();
        edit.key_password = "new-key-pass".to_string();
        edit.ks_type = Some("JKS".to_string());
        edit.sign_v1 = false;
        edit.sign_v2 = true;
        edit.sign_v3 = false;
        edit.sign_v4 = true;
        edit.auto_sign_enabled = false;
        edit.note = "仅更新偏好".to_string();
        edit.set_as_default = true;

        let updated = store
            .update_certificate_preferences(&edit)
            .expect("更新证书偏好失败");

        assert_eq!(updated.name, "生产证书");
        assert!(!updated.sign_v1);
        assert!(updated.sign_v2);
        assert!(!updated.sign_v3);
        assert!(updated.sign_v4);
        assert!(!updated.auto_sign_enabled);
        assert_eq!(updated.note, "仅更新偏好");
        assert!(updated.is_default);

        assert_eq!(updated.source_type, "managed");
        assert_eq!(updated.keystore_path, "/old/release.p12");
        assert_eq!(updated.keystore_password, "old-store-pass");
        assert_eq!(updated.key_alias, "release");
        assert_eq!(updated.key_password, "old-key-pass");
        assert_eq!(updated.ks_type, "PKCS12");
        assert_eq!(updated.last_verified_at, Some(100));
        assert_eq!(updated.last_verify_status, "success");
        assert_eq!(updated.last_verify_message.as_deref(), Some("校验通过"));
    }
}

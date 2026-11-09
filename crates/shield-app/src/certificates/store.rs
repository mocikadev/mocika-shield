//! 单一 SQLite 证书库；连接、SQL 和密文处理不暴露给各入口。
use super::crypto::{decrypt_record_secrets, derive_crypto_key, encrypt_secret};
use super::{
    normalize_keystore_type, CertificatePreferencesPatch, CertificateRecord, CertificateSummary,
    CertificateUpsertInput,
};
use rusqlite::{params, Connection, OptionalExtension};
use std::{
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};
use uuid::Uuid;
const APP_STATE_DEFAULT_CERTIFICATE_ID: &str = "default_certificate_id";
type VerifyState = (String, Option<String>, Option<i64>);
mod binding;
pub use binding::{BoundSigningIdentity, CertificateBinding, CertificateBindingError};

pub struct CertificateStoreState {
    db_path: PathBuf,
    keystore_dir: PathBuf,
    crypto_key: [u8; 32],
    read_only: bool,
}

impl CertificateStoreState {
    /// 入口已定位应用目录后调用，不读取 Tauri 状态，不切换到其他数据目录。
    pub fn initialize(db_path: PathBuf, keystore_dir: PathBuf) -> Result<Self, String> {
        let parent = db_path
            .parent()
            .filter(|path| !path.as_os_str().is_empty())
            .ok_or_else(|| "证书数据库必须位于明确的数据目录".to_string())?;
        fs::create_dir_all(parent).map_err(|e| format!("创建应用数据目录失败: {e}"))?;
        tighten_dir_permissions(parent)?;
        fs::create_dir_all(&keystore_dir).map_err(|e| format!("创建 keystore 目录失败: {e}"))?;
        tighten_dir_permissions(&keystore_dir)?;
        let state = Self::new(db_path, keystore_dir);
        let conn = open_connection(&state.db_path)?;
        initialize_schema(&conn)?;
        tighten_file_permissions(&state.db_path)?;
        Ok(state)
    }
    fn new(db_path: PathBuf, keystore_dir: PathBuf) -> Self {
        let crypto_key = derive_crypto_key(&db_path, &keystore_dir);
        Self {
            db_path,
            keystore_dir,
            crypto_key,
            read_only: false,
        }
    }

    /// 查询入口不创建目录、数据库或模式；文件丢失时也不能退回新建。
    pub fn open_read_only(db_path: PathBuf, keystore_dir: PathBuf) -> Result<Self, String> {
        Self::open_existing_mode(db_path, keystore_dir, true)
    }

    /// 修改既有记录不隐式初始化空库；库被删除后必须停止。
    pub fn open_existing(db_path: PathBuf, keystore_dir: PathBuf) -> Result<Self, String> {
        Self::open_existing_mode(db_path, keystore_dir, false)
    }

    fn open_existing_mode(
        db_path: PathBuf,
        keystore_dir: PathBuf,
        read_only: bool,
    ) -> Result<Self, String> {
        let metadata =
            fs::symlink_metadata(&db_path).map_err(|_| "无法读取既有证书数据库".to_string())?;
        if !db_path.is_absolute() || !metadata.is_file() {
            return Err("证书数据库必须是明确的普通文件".into());
        }
        let mut state = Self::new(db_path, keystore_dir);
        state.read_only = read_only;
        state.open()?;
        Ok(state)
    }

    pub fn keystore_dir(&self) -> &Path {
        &self.keystore_dir
    }

    fn open(&self) -> Result<Connection, String> {
        let flags = if self.read_only {
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY
        } else {
            rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE
        };
        let connection = Connection::open_with_flags(&self.db_path, flags)
            .map_err(|_| "打开既有证书数据库失败，不会自动创建空库".to_string())?;
        connection
            .busy_timeout(std::time::Duration::from_secs(3))
            .map_err(|_| "设置证书数据库等待期限失败".to_string())?;
        Ok(connection)
    }

    pub fn list_certificates(&self) -> Result<Vec<CertificateSummary>, String> {
        let connection = self.open()?;
        let conn = connection
            .unchecked_transaction()
            .map_err(|_| "读取证书快照失败".to_string())?;
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
            .query_map([], row_to_certificate)
            .map_err(|e| format!("读取证书列表失败: {e}"))?;

        let mut items = Vec::new();
        for row in rows {
            let mut item = row.map_err(|e| format!("解析证书记录失败: {e}"))?;
            item.is_default = default_id
                .as_deref()
                .is_some_and(|default_id| default_id == item.id);
            items.push(item.into());
        }
        Ok(items)
    }

    pub fn get_certificate(&self, id: &str) -> Result<Option<CertificateRecord>, String> {
        let connection = self.open()?;
        let conn = connection
            .unchecked_transaction()
            .map_err(|_| "读取证书快照失败".to_string())?;
        let default_id = get_default_certificate_id(&conn)?;
        let Some(mut item) = read_encrypted_certificate(&conn, id)? else {
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

    pub fn save_certificate(
        &self,
        input: &CertificateUpsertInput,
        keystore_path: &str,
        resolved_alias: &str,
        verify_status: Option<(&str, Option<&str>, i64)>,
    ) -> Result<CertificateRecord, String> {
        if input.id.is_some() {
            return Err("新增证书不能指定已有 ID，请使用证书偏好更新或重新导入".to_string());
        }
        let mut conn = self.open()?;
        let tx = conn
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
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

    pub fn set_default_certificate(&self, id: Option<&str>) -> Result<(), String> {
        let mut conn = self.open()?;
        let tx = conn
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
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

    pub fn update_certificate_preferences(
        &self,
        input: &CertificateUpsertInput,
    ) -> Result<CertificateRecord, String> {
        let id = input
            .id
            .as_deref()
            .ok_or_else(|| "缺少要更新的证书 ID".to_string())?;
        self.patch_certificate_preferences(
            id,
            CertificatePreferencesPatch {
                name: Some(input.name.clone()),
                note: Some(input.note.clone()),
                sign_v1: Some(input.sign_v1),
                sign_v2: Some(input.sign_v2),
                sign_v3: Some(input.sign_v3),
                sign_v4: Some(input.sign_v4),
                auto_sign_enabled: Some(input.auto_sign_enabled),
            },
            input.set_as_default,
        )?;
        self.get_certificate(id)?
            .ok_or_else(|| "更新证书后未能重新读取记录".to_string())
    }

    pub fn patch_certificate_preferences(
        &self,
        id: &str,
        patch: CertificatePreferencesPatch,
        set_as_default: bool,
    ) -> Result<CertificateSummary, String> {
        let mut conn = self.open()?;
        let tx = conn
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(|e| format!("创建证书更新事务失败: {e}"))?;
        let now = now_timestamp();

        let changed = tx
            .execute(
                "UPDATE certificates
             SET name = CASE WHEN ?2 IS NULL THEN name
                        WHEN ?2 = '' THEN key_alias ELSE ?2 END,
                 sign_v1 = COALESCE(?3, sign_v1),
                 sign_v2 = COALESCE(?4, sign_v2),
                 sign_v3 = COALESCE(?5, sign_v3),
                 sign_v4 = COALESCE(?6, sign_v4),
                 auto_sign_enabled = COALESCE(?7, auto_sign_enabled),
                 note = COALESCE(?8, note),
                 updated_at = ?9
             WHERE id = ?1",
                params![
                    id,
                    patch.name.as_deref().map(str::trim),
                    patch.sign_v1,
                    patch.sign_v2,
                    patch.sign_v3,
                    patch.sign_v4,
                    patch.auto_sign_enabled,
                    patch.note.as_deref().map(str::trim),
                    now,
                ],
            )
            .map_err(|e| format!("更新证书偏好失败: {e}"))?;
        if changed != 1 {
            return Err("要更新的证书不存在".into());
        }
        if set_as_default {
            set_default_certificate_id_tx(&tx, Some(id))?;
        }

        tx.commit().map_err(|e| format!("提交证书更新失败: {e}"))?;
        self.list_certificates()?
            .into_iter()
            .find(|item| item.id == id)
            .ok_or_else(|| "更新证书后未能重新读取摘要".to_string())
    }

    pub fn delete_certificate(&self, id: &str, remove_keystore_file: bool) -> Result<(), String> {
        let record = self
            .list_certificates()?
            .into_iter()
            .find(|item| item.id == id)
            .ok_or_else(|| "要删除的证书不存在".to_string())?;
        let mut conn = self.open()?;
        let tx = conn
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(|e| format!("创建删除证书事务失败: {e}"))?;
        let changed = tx
            .execute("DELETE FROM certificates WHERE id = ?1", [id])
            .map_err(|e| format!("删除证书记录失败: {e}"))?;
        if changed != 1 {
            return Err("要删除的证书不存在".into());
        }
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

    pub fn update_verify_status(
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

fn read_encrypted_certificate(
    conn: &Connection,
    id: &str,
) -> Result<Option<CertificateRecord>, String> {
    conn.query_row(
        "SELECT id, name, source_type, keystore_path, keystore_password, key_alias, \
         key_password, ks_type, sign_v1, sign_v2, sign_v3, sign_v4, auto_sign_enabled, \
         note, created_at, updated_at, last_verified_at, last_verify_status, last_verify_message \
         FROM certificates WHERE id = ?1",
        [id],
        row_to_certificate,
    )
    .optional()
    .map_err(|_| "读取证书详情失败".to_string())
}

fn row_to_certificate(row: &rusqlite::Row<'_>) -> rusqlite::Result<CertificateRecord> {
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
}

fn open_connection(path: &Path) -> Result<Connection, String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("创建数据库目录失败: {e}"))?;
    }
    let conn = Connection::open(path).map_err(|e| format!("打开证书数据库失败: {e}"))?;
    conn.busy_timeout(std::time::Duration::from_secs(3))
        .map_err(|e| format!("设置证书数据库等待期限失败: {e}"))?;
    Ok(conn)
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
#[path = "store_tests.rs"]
mod tests;

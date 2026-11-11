//! 证书导入、创建与校验用例，各入口共用一套业务规则。
use super::managed;
use super::normalize_keystore_type;
use super::query_keystore_aliases;
use super::CertificateError;
use super::{
    CertificateRecord, CertificateSelection, CertificateStoreState, CertificateUpsertInput,
    CertificateValidationInput, CertificateValidationResult,
};
use shield_core::diagnostic::{Diagnostic, FailureCode};
use std::fs;
use std::path::PathBuf;

pub struct CertificateOutcome<T> {
    pub value: T,
    pub diagnostic: Option<Diagnostic>,
}

/// 本次选择立即解析为材料快照；明确编号失效时绝不回退默认项。
pub fn resolve_signing_certificate(
    store: &CertificateStoreState,
    selection: CertificateSelection<'_>,
) -> Result<CertificateRecord, CertificateError> {
    store
        .capture_signing_identity(selection)
        .map(|identity| identity.into_parts().0)
        .map_err(|error| preflight_input(&error.to_string()))
}

impl<T> CertificateOutcome<T> {
    fn plain(value: T) -> Self {
        Self {
            value,
            diagnostic: None,
        }
    }
}

pub fn validate_certificate_input(
    input: CertificateValidationInput,
) -> Result<CertificateOutcome<CertificateValidationResult>, CertificateError> {
    let path = input.keystore_path.trim();
    if path.is_empty() {
        return Ok(CertificateOutcome::plain(CertificateValidationResult {
            valid: false,
            aliases: Vec::new(),
            resolved_alias: None,
            message: Some("请选择 Keystore 文件".to_string()),
        }));
    }
    if input.keystore_password.trim().is_empty() {
        return Ok(CertificateOutcome::plain(CertificateValidationResult {
            valid: false,
            aliases: Vec::new(),
            resolved_alias: None,
            message: Some("请输入 Keystore 密码".to_string()),
        }));
    }

    let aliases = query_keystore_aliases(
        path.to_string(),
        input.keystore_password.clone(),
        input.ks_type.clone(),
    )?;
    Ok(validation_from_aliases(aliases, &input.key_alias))
}

fn validation_from_aliases(
    aliases: Vec<String>,
    requested_alias: &str,
) -> CertificateOutcome<CertificateValidationResult> {
    let Some(actual_alias) = resolve_alias(&aliases, requested_alias) else {
        let missing_requested = !requested_alias.trim().is_empty();
        return CertificateOutcome {
            value: CertificateValidationResult {
                valid: false,
                aliases,
                resolved_alias: None,
                message: Some(if missing_requested {
                    "未在 keystore 中找到指定 alias".to_string()
                } else {
                    "请输入 Key Alias，或只保留一个 alias 后再自动识别".to_string()
                }),
            },
            diagnostic: missing_requested.then(|| Diagnostic::new(FailureCode::KeyAliasNotFound)),
        };
    };
    CertificateOutcome::plain(CertificateValidationResult {
        valid: true,
        aliases,
        resolved_alias: Some(actual_alias),
        message: None,
    })
}

fn resolve_alias(aliases: &[String], requested_alias: &str) -> Option<String> {
    let alias = requested_alias.trim();
    if alias.is_empty() {
        return (aliases.len() == 1).then(|| aliases[0].clone());
    }

    aliases
        .iter()
        .find(|item| item.eq_ignore_ascii_case(alias))
        .cloned()
}

pub fn save_certificate_profile(
    store: &CertificateStoreState,
    input: CertificateUpsertInput,
) -> Result<CertificateRecord, CertificateError> {
    if input.id.is_some() {
        return store
            .update_certificate_preferences(&input)
            .map_err(Into::into);
    }

    let validation = validate_certificate_input(CertificateValidationInput {
        keystore_path: input.keystore_path.clone(),
        keystore_password: input.keystore_password.clone(),
        key_alias: input.key_alias.clone(),
        ks_type: input.ks_type.clone(),
    })?;
    if !validation.value.valid {
        let message = validation
            .value
            .message
            .unwrap_or_else(|| "证书校验失败，无法保存".to_string());
        return Err(match validation.diagnostic {
            Some(diagnostic) => CertificateError::diagnosed(message, diagnostic),
            None => CertificateError::preflight(message, Diagnostic::new(FailureCode::Unknown)),
        });
    }
    let resolved_alias = validation
        .value
        .resolved_alias
        .clone()
        .ok_or_else(|| "校验通过但未解析到 alias".to_string())?;
    save_validated_certificate_profile(store, input, &resolved_alias)
}

pub(super) fn save_validated_certificate_profile(
    store: &CertificateStoreState,
    input: CertificateUpsertInput,
    resolved_alias: &str,
) -> Result<CertificateRecord, CertificateError> {
    let final_path = resolve_keystore_path_for_save(store, &input)?;
    let now = current_timestamp();
    store
        .save_certificate(
            &input,
            &final_path,
            resolved_alias,
            Some(("success", None, now)),
        )
        .map_err(|error| {
            if normalize_source_type(&input.source_type) == "managed"
                && input.copy_keystore_to_managed
            {
                CertificateError::from(
                    "证书记录保存失败，已生成的托管证书文件保留在证书目录中，请检查后再处理"
                        .to_string(),
                )
            } else {
                CertificateError::from(error)
            }
        })
}

pub fn verify_saved_certificate(
    store: &CertificateStoreState,
    id: &str,
) -> Result<CertificateOutcome<CertificateRecord>, CertificateError> {
    let record = store
        .get_certificate(id)?
        .ok_or_else(|| "未找到要校验的证书".to_string())?;
    let validation = validate_certificate_input(CertificateValidationInput {
        keystore_path: record.keystore_path.clone(),
        keystore_password: record.keystore_password.clone(),
        key_alias: record.key_alias.clone(),
        ks_type: Some(record.ks_type.clone()),
    });
    match validation {
        Ok(result) if result.value.valid => {
            let alias = result.value.resolved_alias.as_deref();
            store
                .update_verify_status(id, "success", None, alias)
                .map(CertificateOutcome::plain)
                .map_err(Into::into)
        }
        Ok(result) => store
            .update_verify_status(id, "failed", result.value.message.as_deref(), None)
            .map(|value| CertificateOutcome {
                value,
                diagnostic: result.diagnostic,
            })
            .map_err(Into::into),
        Err(err) => {
            let value = store.update_verify_status(id, "failed", Some(&err.message), None)?;
            Ok(CertificateOutcome {
                value,
                diagnostic: Some(*err.diagnostic),
            })
        }
    }
}

pub(super) fn preflight_input(message: &str) -> CertificateError {
    CertificateError::preflight(message.to_string(), Diagnostic::new(FailureCode::Unknown))
}

fn resolve_keystore_path_for_save(
    store: &CertificateStoreState,
    input: &CertificateUpsertInput,
) -> Result<String, String> {
    if normalize_source_type(&input.source_type) == "managed" && input.copy_keystore_to_managed {
        let source = PathBuf::from(input.keystore_path.trim());
        if !source.exists() {
            return Err("要托管的 keystore 文件不存在".to_string());
        }
        let requested = input
            .managed_file_name
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .map(|value| build_managed_file_name(value, input.ks_type.as_deref()))
            .unwrap_or_else(|| {
                build_managed_file_name(
                    source
                        .file_name()
                        .and_then(|value| value.to_str())
                        .unwrap_or("keystore.jks"),
                    input.ks_type.as_deref(),
                )
            });
        let stage = managed::draft(store.keystore_dir())?;
        let staged = stage.path().join("imported.keystore");
        fs::copy(&source, &staged).map_err(|e| format!("复制 keystore 到暂存目录失败: {e}"))?;
        let target = managed::publish(&staged, store.keystore_dir(), &requested)?;
        return Ok(target.to_string_lossy().to_string());
    }

    Ok(input.keystore_path.trim().to_string())
}

pub(super) fn build_managed_file_name(value: &str, ks_type: Option<&str>) -> String {
    let mut stem = sanitize_file_name(value);
    if stem.is_empty() {
        stem = "keystore".to_string();
    }
    let ext = match normalize_keystore_type(ks_type).as_deref() {
        Some("PKCS12") => "p12",
        _ => "jks",
    };
    if stem.ends_with(".jks") || stem.ends_with(".p12") || stem.ends_with(".keystore") {
        stem
    } else {
        format!("{stem}.{ext}")
    }
}

fn sanitize_file_name(value: &str) -> String {
    value
        .trim()
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' || ch == '.' {
                ch
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_string()
}

fn normalize_source_type(value: &str) -> &'static str {
    if value == "managed" {
        "managed"
    } else {
        "external"
    }
}

fn current_timestamp() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

#[cfg(test)]
mod tests {
    use super::{resolve_alias, validation_from_aliases};
    use shield_core::diagnostic::FailureCode;

    fn aliases(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    #[test]
    fn 不存在的_alias_保留验证载荷并单独返回诊断() {
        let outcome = validation_from_aliases(aliases(&["release", "debug"]), "missing");
        assert!(!outcome.value.valid);
        assert_eq!(outcome.value.aliases, aliases(&["release", "debug"]));
        assert_eq!(
            outcome.diagnostic.map(|item| item.code),
            Some(FailureCode::KeyAliasNotFound)
        );
    }

    #[test]
    fn alias_输入为空且只有一个_alias_时自动识别() {
        assert_eq!(
            resolve_alias(&aliases(&["release"]), ""),
            Some("release".to_string())
        );
        assert_eq!(
            resolve_alias(&aliases(&["release"]), "  "),
            Some("release".to_string())
        );
    }

    #[test]
    fn alias_输入为空且存在多个_alias_时不自动选择() {
        assert_eq!(resolve_alias(&aliases(&["debug", "release"]), ""), None);
    }

    #[test]
    fn alias_按大小写不敏感匹配并返回实际_alias() {
        assert_eq!(
            resolve_alias(&aliases(&["releasealias"]), "ReleaseAlias"),
            Some("releasealias".to_string())
        );
        assert_eq!(
            resolve_alias(&aliases(&["ReleaseAlias"]), "releasealias"),
            Some("ReleaseAlias".to_string())
        );
    }

    #[test]
    fn alias_不存在时返回空() {
        assert_eq!(resolve_alias(&aliases(&["release"]), "debug"), None);
    }
}

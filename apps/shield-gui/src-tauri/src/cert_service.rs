use crate::app_config::normalize_keystore_type;
use crate::cert_store::{
    CertificateRecord, CertificateStoreState, CertificateUpsertInput, CertificateValidationInput,
    CertificateValidationResult, CreateManagedCertificateInput,
};
use crate::signing::query_keystore_aliases;
use shield_core::utils::{find_keytool, no_window_command};
use std::fs;
use std::path::{Path, PathBuf};

pub(crate) fn validate_certificate_input(
    input: CertificateValidationInput,
) -> Result<CertificateValidationResult, String> {
    let path = input.keystore_path.trim();
    if path.is_empty() {
        return Ok(CertificateValidationResult {
            valid: false,
            aliases: Vec::new(),
            resolved_alias: None,
            message: Some("请选择 Keystore 文件".to_string()),
        });
    }
    if input.keystore_password.trim().is_empty() {
        return Ok(CertificateValidationResult {
            valid: false,
            aliases: Vec::new(),
            resolved_alias: None,
            message: Some("请输入 Keystore 密码".to_string()),
        });
    }

    let aliases = query_keystore_aliases(
        path.to_string(),
        input.keystore_password.clone(),
        input.ks_type.clone(),
    )?;
    let Some(actual_alias) = resolve_alias(&aliases, &input.key_alias) else {
        let message = if input.key_alias.trim().is_empty() {
            "请输入 Key Alias，或只保留一个 alias 后再自动识别"
        } else {
            "未在 keystore 中找到指定 alias"
        };
        return Ok(CertificateValidationResult {
            valid: false,
            aliases,
            resolved_alias: None,
            message: Some(message.to_string()),
        });
    };

    Ok(CertificateValidationResult {
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

pub(crate) fn save_certificate_profile(
    store: &CertificateStoreState,
    input: CertificateUpsertInput,
) -> Result<CertificateRecord, String> {
    if input.id.is_some() {
        return store.update_certificate_preferences(&input);
    }

    let validation = validate_certificate_input(CertificateValidationInput {
        keystore_path: input.keystore_path.clone(),
        keystore_password: input.keystore_password.clone(),
        key_alias: input.key_alias.clone(),
        ks_type: input.ks_type.clone(),
    })?;
    if !validation.valid {
        return Err(validation
            .message
            .unwrap_or_else(|| "证书校验失败，无法保存".to_string()));
    }
    let resolved_alias = validation
        .resolved_alias
        .clone()
        .ok_or_else(|| "校验通过但未解析到 alias".to_string())?;
    let final_path = resolve_keystore_path_for_save(store, &input)?;
    let now = current_timestamp();
    store.save_certificate(
        &input,
        &final_path,
        &resolved_alias,
        Some(("success", None, now)),
    )
}

pub(crate) fn verify_saved_certificate(
    store: &CertificateStoreState,
    id: &str,
) -> Result<CertificateRecord, String> {
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
        Ok(result) if result.valid => {
            let alias = result.resolved_alias.as_deref();
            store.update_verify_status(id, "success", None, alias)
        }
        Ok(result) => store.update_verify_status(id, "failed", result.message.as_deref(), None),
        Err(err) => store.update_verify_status(id, "failed", Some(&err), None),
    }
}

pub(crate) fn create_managed_certificate(
    store: &CertificateStoreState,
    input: CreateManagedCertificateInput,
) -> Result<CertificateRecord, String> {
    if input.name.trim().is_empty() {
        return Err("请输入证书名称".to_string());
    }
    if input.key_alias.trim().is_empty() {
        return Err("请输入 Key Alias".to_string());
    }
    if input.keystore_password.trim().is_empty() {
        return Err("请输入 Keystore 密码".to_string());
    }
    if input.keystore_password.trim().chars().count() < 6 {
        return Err("Keystore 密码至少需要 6 个字符".to_string());
    }
    if !input.key_password.trim().is_empty() && input.key_password.trim().chars().count() < 6 {
        return Err("Key 密码至少需要 6 个字符".to_string());
    }
    if input.dname.trim().is_empty() {
        return Err("请输入证书主题信息".to_string());
    }
    if input.validity_days == 0 {
        return Err("有效期必须大于 0 天".to_string());
    }
    if input.key_size != 2048 && input.key_size != 4096 {
        return Err("密钥位数建议使用 2048 或 4096".to_string());
    }

    let file_name = build_managed_file_name(&input.file_name, input.ks_type.as_deref());
    let keystore_path = allocate_unique_path(store.keystore_dir(), &file_name);
    let ks_type =
        normalize_keystore_type(input.ks_type.as_deref()).unwrap_or_else(|| "JKS".to_string());
    let keytool = find_keytool().map_err(|err| err.to_string())?;
    let effective_key_password = if input.key_password.trim().is_empty() {
        input.keystore_password.clone()
    } else {
        input.key_password.clone()
    };

    let output = no_window_command(&keytool)
        .args([
            "-genkeypair",
            "-keystore",
            keystore_path.to_string_lossy().as_ref(),
            "-storetype",
            &ks_type,
            "-storepass",
            &input.keystore_password,
            "-alias",
            &input.key_alias,
            "-keypass",
            &effective_key_password,
            "-dname",
            input.dname.trim(),
            "-validity",
            &input.validity_days.to_string(),
            "-keyalg",
            "RSA",
            "-keysize",
            &input.key_size.to_string(),
        ])
        .output()
        .map_err(|e| format!("启动 keytool 失败，请确认 JDK 8+ 已安装: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(classify_create_keystore_error(&stderr));
    }

    let upsert = CertificateUpsertInput {
        id: None,
        name: input.name,
        source_type: "managed".to_string(),
        keystore_path: keystore_path.to_string_lossy().to_string(),
        keystore_password: input.keystore_password,
        key_alias: input.key_alias,
        key_password: input.key_password,
        ks_type: Some(ks_type),
        sign_v1: input.sign_v1,
        sign_v2: input.sign_v2,
        sign_v3: input.sign_v3,
        sign_v4: input.sign_v4,
        auto_sign_enabled: input.auto_sign_enabled,
        note: input.note,
        set_as_default: input.set_as_default,
        copy_keystore_to_managed: false,
        managed_file_name: None,
    };

    save_certificate_profile(store, upsert)
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
        let target = allocate_unique_path(store.keystore_dir(), &requested);
        fs::copy(&source, &target).map_err(|e| format!("复制 keystore 到托管目录失败: {e}"))?;
        return Ok(target.to_string_lossy().to_string());
    }

    Ok(input.keystore_path.trim().to_string())
}

fn build_managed_file_name(value: &str, ks_type: Option<&str>) -> String {
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

fn allocate_unique_path(base: &Path, file_name: &str) -> PathBuf {
    let candidate = base.join(file_name);
    if !candidate.exists() {
        return candidate;
    }

    let stem = Path::new(file_name)
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("keystore");
    let ext = Path::new(file_name)
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("");

    for index in 2..1000 {
        let name = if ext.is_empty() {
            format!("{stem}-{index}")
        } else {
            format!("{stem}-{index}.{ext}")
        };
        let next = base.join(name);
        if !next.exists() {
            return next;
        }
    }

    base.join(format!("{}-{}.{}", stem, current_timestamp(), ext))
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

fn classify_create_keystore_error(stderr: &str) -> String {
    let raw = stderr.trim();
    let lower = raw.to_lowercase();

    let reason = if lower.contains("alias") && lower.contains("already exists")
        || raw.contains("别名") && raw.contains("已经存在")
        || raw.contains("别名") && raw.contains("已存在")
    {
        "证书 Alias 已存在，请换一个 Alias 或文件名"
    } else if lower.contains("key password must be at least 6 characters")
        || lower.contains("password must be at least 6 characters")
        || lower.contains("password is too short")
        || raw.contains("密码至少")
        || raw.contains("口令至少")
    {
        "证书密码不符合 keytool 要求，Keystore 密码和 Key 密码至少需要 6 个字符"
    } else if lower.contains("incorrect avas format")
        || lower.contains("invalid name")
        || lower.contains("distinguished name")
        || raw.contains("专有名称")
        || raw.contains("名称无效")
    {
        "证书主题信息格式不正确，请检查 CN、OU、O、L、ST、C 等字段"
    } else if lower.contains("permission denied")
        || lower.contains("access is denied")
        || raw.contains("权限")
        || raw.contains("拒绝访问")
    {
        "没有权限写入证书文件，请检查应用数据目录权限"
    } else if lower.contains("no such file")
        || lower.contains("cannot find")
        || raw.contains("没有那个文件")
        || raw.contains("系统找不到")
    {
        "证书保存目录不存在或不可访问"
    } else {
        "创建证书失败，请检查证书名称、Alias、密码和主题信息"
    };

    if raw.is_empty() {
        reason.to_string()
    } else {
        format!("{reason}。keytool 输出：{raw}")
    }
}

#[cfg(test)]
mod tests {
    use super::{classify_create_keystore_error, resolve_alias};

    fn aliases(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
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

    #[test]
    fn 创建证书_alias_已存在提示更明确() {
        let message = classify_create_keystore_error(
            "keytool error: java.lang.Exception: Alias <release> already exists",
        );
        assert!(message.starts_with("证书 Alias 已存在"));
    }

    #[test]
    fn 创建证书_密码过短提示更明确() {
        let message = classify_create_keystore_error(
            "keytool error: Key password must be at least 6 characters",
        );
        assert!(message.starts_with("证书密码不符合 keytool 要求"));
    }

    #[test]
    fn 创建证书_dname_非法提示更明确() {
        let message = classify_create_keystore_error(
            "keytool error: java.io.IOException: Incorrect AVAs format",
        );
        assert!(message.starts_with("证书主题信息格式不正确"));
    }

    #[test]
    fn 创建证书_目录权限错误提示更明确() {
        let message = classify_create_keystore_error(
            "keytool error: java.io.FileNotFoundException: Permission denied",
        );
        assert!(message.starts_with("没有权限写入证书文件"));
    }

    #[test]
    fn 创建证书_未知错误保留原始输出摘要() {
        let message = classify_create_keystore_error("keytool error: unknown failure");
        assert!(message.starts_with("创建证书失败"));
        assert!(message.contains("unknown failure"));
    }
}

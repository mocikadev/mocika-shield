//! 新建托管证书用例；暂存／发布／入库在服务层，工具参数及核验在核心层。
use super::service::{
    build_managed_file_name, preflight_input, save_certificate_profile,
    save_validated_certificate_profile,
};
use super::{
    managed, normalize_keystore_type, CertificateError, CertificateRecord, CertificateStoreState,
    CertificateUpsertInput, CreateManagedCertificateInput,
};
use shield_core::{diagnostic::Diagnostic, JavaToolContext};
use std::path::Path;

/// Windows 普通新建使用默认适配，其他平台保留原工具路径；共用发布及保存规则。
pub fn create_managed_certificate(
    store: &CertificateStoreState,
    input: CreateManagedCertificateInput,
) -> Result<CertificateRecord, CertificateError> {
    create_using(store, input, None)
}

/// 显式接入已选资源、Java 和 keytool；只保存完整核验成功的新证书。
pub fn create_managed_certificate_with_context(
    store: &CertificateStoreState,
    input: CreateManagedCertificateInput,
    context: &JavaToolContext,
    java: &Path,
    keytool: &Path,
) -> Result<CertificateRecord, CertificateError> {
    create_using(store, input, Some((context, java, keytool)))
}

fn create_using(
    store: &CertificateStoreState,
    input: CreateManagedCertificateInput,
    adapter: Option<(
        &shield_core::JavaToolContext,
        &std::path::Path,
        &std::path::Path,
    )>,
) -> Result<CertificateRecord, CertificateError> {
    if input.name.trim().is_empty() {
        return Err(preflight_input("请输入证书名称"));
    }
    if input.key_alias.trim().is_empty() {
        return Err(preflight_input("请输入 Key Alias"));
    }
    if input.keystore_password.trim().is_empty() {
        return Err(preflight_input("请输入 Keystore 密码"));
    }
    if input.keystore_password.trim().chars().count() < 6 {
        return Err(preflight_input("Keystore 密码至少需要 6 个字符"));
    }
    if !input.key_password.trim().is_empty() && input.key_password.trim().chars().count() < 6 {
        return Err(preflight_input("Key 密码至少需要 6 个字符"));
    }
    if input.dname.trim().is_empty() {
        return Err(preflight_input("请输入证书主题信息"));
    }
    if input.validity_days == 0 {
        return Err(preflight_input("有效期必须大于 0 天"));
    }
    if input.key_size != 2048 && input.key_size != 4096 {
        return Err(preflight_input("密钥位数建议使用 2048 或 4096"));
    }

    let file_name = build_managed_file_name(&input.file_name, input.ks_type.as_deref());
    let stage = managed::draft(store.keystore_dir())?;
    let directory = if adapter.is_some() || cfg!(windows) {
        managed::adapter_directory(stage.path())?
    } else {
        stage.path().to_path_buf()
    };
    let keystore_path = directory.join(&file_name);
    let ks_type =
        normalize_keystore_type(input.ks_type.as_deref()).unwrap_or_else(|| "JKS".to_string());
    let options = shield_core::keytool::CreateKeystoreOptions {
        kind: &ks_type,
        alias: &input.key_alias,
        keystore_password: &input.keystore_password,
        key_password: &input.key_password,
        dname: input.dname.trim(),
        validity_days: input.validity_days,
        key_size: input.key_size,
    };
    let actual = match adapter {
        Some((context, java, keytool)) => Some(
            shield_core::keytool::create_keystore_draft_with_context(
                context,
                java,
                keytool,
                &keystore_path,
                &options,
            )
            .map_err(|error| {
                CertificateError::diagnosed(error.to_string(), Diagnostic::from_error(&error))
            })?
            .alias,
        ),
        #[cfg(windows)]
        None => Some(
            shield_core::keytool::create_keystore_draft(&keystore_path, &options)
                .map_err(|error| {
                    CertificateError::diagnosed(error.to_string(), Diagnostic::from_error(&error))
                })?
                .alias,
        ),
        #[cfg(not(windows))]
        None => {
            shield_core::keytool::generate_keystore_draft(&keystore_path, &options).map_err(
                |error| {
                    CertificateError::diagnosed(error.to_string(), Diagnostic::from_error(&error))
                },
            )?;
            None
        }
    };

    let mut upsert = CertificateUpsertInput {
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
        copy_keystore_to_managed: true,
        managed_file_name: Some(file_name.clone()),
    };

    match actual {
        Some(alias) => {
            // 新入口直接无覆盖发布已核验的私有材料，避免复制到继承宽权限的中转文件。
            let published = managed::publish(&keystore_path, store.keystore_dir(), &file_name)?;
            upsert.keystore_path = published.to_string_lossy().into_owned();
            upsert.key_alias = alias.clone();
            upsert.copy_keystore_to_managed = false;
            save_validated_certificate_profile(store, upsert, &alias).map_err(|_| {
                CertificateError::from(
                    "证书记录保存失败，新证书保留在证书目录中，请检查后再处理".to_string(),
                )
            })
        }
        None => save_certificate_profile(store, upsert),
    }
}

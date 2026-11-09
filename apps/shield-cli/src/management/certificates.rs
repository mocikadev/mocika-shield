use super::{args::*, credentials, initialize_store, writable_store, ManagementError, Response};
use serde_json::json;
use shield_app::{
    certificates::{
        create_managed_certificate, save_certificate_profile, verify_saved_certificate,
        CertificateError, CertificatePreferencesPatch, CertificateStoreState, CertificateSummary,
        CertificateUpsertInput, CreateManagedCertificateInput,
    },
    paths::ApplicationPaths,
};

pub(super) fn run(command: CertificateCommand, paths: &ApplicationPaths) -> Response {
    match command {
        CertificateCommand::List => Ok(json!({"certificates": list(paths)?})),
        CertificateCommand::Show { id } => Ok(json!({"certificate": find(paths, &id)?})),
        CertificateCommand::Import(args) => import(args, paths),
        CertificateCommand::Create(args) => create(args, paths),
        CertificateCommand::Edit(args) => edit(args, paths),
        CertificateCommand::Verify { id } => {
            find(paths, &id)?;
            let result = verify_saved_certificate(&writable_store(paths)?, &id)
                .map_err(|error| certificate_error(error).after_write())?;
            if result.value.last_verify_status != "success" {
                return Err(ManagementError::new(
                    result
                        .diagnostic
                        .map_or("CERTIFICATE_INVALID", |diagnostic| diagnostic.code.as_str()),
                    "已保存失败校验结果；请在本机检查证书文件、密码、Alias 和 keytool。",
                )
                .verified_failure(id));
            }
            Ok(json!({"certificate": CertificateSummary::from(result.value)}))
        }
        CertificateCommand::SetDefault { id } => {
            find(paths, &id)?;
            writable_store(paths)?
                .set_default_certificate(Some(&id))
                .map_err(|error| ManagementError::store(error).after_write())?;
            Ok(json!({"default_certificate_id": id}))
        }
        CertificateCommand::ClearDefault => {
            // 先验证既有库；空库也可能有待清理的默认编号，不能只依据列表长度。
            list(paths)?;
            if paths.certificate_database().is_file() {
                writable_store(paths)?
                    .set_default_certificate(None)
                    .map_err(|error| ManagementError::store(error).after_write())?;
            }
            Ok(json!({"default_certificate_id": null}))
        }
        CertificateCommand::Remove { id, yes } => {
            if !yes {
                return Err(ManagementError::new(
                    "CONFIRMATION_REQUIRED",
                    "删除证书记录须提供 --yes；证书文件始终保留。",
                ));
            }
            find(paths, &id)?;
            writable_store(paths)?
                .delete_certificate(&id, false)
                .map_err(|error| ManagementError::store(error).after_write())?;
            Ok(json!({"removed_id": id, "keystore_file": "retained"}))
        }
    }
}

fn list(paths: &ApplicationPaths) -> Result<Vec<CertificateSummary>, ManagementError> {
    let path = paths.certificate_database();
    match std::fs::symlink_metadata(&path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(_) => {
            return Err(ManagementError::new(
                "CERTIFICATE_STORE_FAILED",
                "无法读取证书库；不会切换到其他目录。",
            ))
        }
        Ok(_) => {}
    }
    CertificateStoreState::open_read_only(path, paths.keystore_dir())
        .map_err(ManagementError::store)?
        .list_certificates()
        .map_err(ManagementError::store)
}
pub(super) fn find(
    paths: &ApplicationPaths,
    id: &str,
) -> Result<CertificateSummary, ManagementError> {
    list(paths)?
        .into_iter()
        .find(|item| item.id == id)
        .ok_or_else(|| {
            ManagementError::new(
                "CERTIFICATE_NOT_FOUND",
                "指定证书不存在；不会改用默认证书。",
            )
        })
}

fn import(args: ImportArgs, paths: &ApplicationPaths) -> Response {
    let secret = credentials::read()?;
    let path = if args.keystore.is_absolute() {
        args.keystore
    } else {
        std::env::current_dir()
            .map_err(|_| ManagementError::new("INPUT_INVALID", "无法解析本次证书路径。"))?
            .join(args.keystore)
    };
    // 不 canonicalize 现有材料路径；编码不支持时拒绝，不能有损转换后保存错误文件名。
    let path = path
        .to_str()
        .ok_or_else(|| ManagementError::new("INPUT_INVALID", "证书路径编码不受支持。"))?;
    let input = CertificateUpsertInput {
        id: None,
        name: args.name,
        source_type: "external".into(),
        keystore_path: path.into(),
        keystore_password: secret.keystore,
        key_alias: args.alias,
        key_password: secret.key,
        ks_type: Some(args.keystore_type),
        sign_v1: args.signing.v1.unwrap_or(true),
        sign_v2: args.signing.v2.unwrap_or(true),
        sign_v3: args.signing.v3.unwrap_or(true),
        sign_v4: args.signing.v4.unwrap_or(false),
        auto_sign_enabled: args.signing.auto_sign.unwrap_or(true),
        note: args.note,
        set_as_default: args.set_default,
        copy_keystore_to_managed: false,
        managed_file_name: None,
    };
    let value = save_certificate_profile(&initialize_store(paths)?, input)
        .map_err(|error| certificate_error(error).after_write())?;
    Ok(json!({"certificate": CertificateSummary::from(value)}))
}
fn create(args: CreateArgs, paths: &ApplicationPaths) -> Response {
    // CLI 只接受文件名；托管目录和同名无覆盖分配仍由共享服务负责。
    if args.file_name.is_empty()
        || args.file_name.contains(['/', '\\'])
        || [".", ".."].contains(&args.file_name.as_str())
    {
        return Err(ManagementError::new(
            "INPUT_INVALID",
            "托管证书只接受文件名，不接受目录。",
        ));
    }
    let secret = credentials::read()?;
    let input = CreateManagedCertificateInput {
        name: args.name,
        file_name: args.file_name,
        key_alias: args.alias,
        keystore_password: secret.keystore,
        key_password: secret.key,
        ks_type: Some(args.keystore_type),
        sign_v1: args.signing.v1.unwrap_or(true),
        sign_v2: args.signing.v2.unwrap_or(true),
        sign_v3: args.signing.v3.unwrap_or(true),
        sign_v4: args.signing.v4.unwrap_or(false),
        auto_sign_enabled: args.signing.auto_sign.unwrap_or(true),
        note: args.note,
        set_as_default: args.set_default,
        dname: args.dname,
        validity_days: args.validity_days,
        key_size: args.key_size,
    };
    let value = create_managed_certificate(&initialize_store(paths)?, input)
        .map_err(|error| certificate_error(error).after_write())?;
    Ok(json!({"certificate": CertificateSummary::from(value)}))
}
fn edit(args: EditArgs, paths: &ApplicationPaths) -> Response {
    let patch = CertificatePreferencesPatch {
        name: args.name,
        note: args.note,
        sign_v1: args.signing.v1,
        sign_v2: args.signing.v2,
        sign_v3: args.signing.v3,
        sign_v4: args.signing.v4,
        auto_sign_enabled: args.signing.auto_sign,
    };
    if patch.name.is_none()
        && patch.note.is_none()
        && patch.sign_v1.is_none()
        && patch.sign_v2.is_none()
        && patch.sign_v3.is_none()
        && patch.sign_v4.is_none()
        && patch.auto_sign_enabled.is_none()
    {
        return Err(ManagementError::new(
            "PATCH_REQUIRED",
            "请明确指定需要修改的证书偏好。",
        ));
    }
    find(paths, &args.id)?;
    let value = writable_store(paths)?
        .patch_certificate_preferences(&args.id, patch, false)
        .map_err(|error| ManagementError::store(error).after_write())?;
    Ok(json!({"certificate": value}))
}
fn certificate_error(error: CertificateError) -> ManagementError {
    ManagementError::new(error.diagnostic.code.as_str(), "证书操作未完成；请在本机检查输入、证书和 keytool。不会输出密码或原始工具日志，修改结果不明时请先查询记录再重试。")
}

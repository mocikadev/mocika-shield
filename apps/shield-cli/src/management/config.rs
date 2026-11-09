use super::{args::ConfigCommand, ManagementError, Response};
use serde_json::{json, Map, Value};
use shield_app::{
    config::{ConfigStore, ProtectDefaultsPatch},
    paths::ApplicationPaths,
};

pub(super) fn run(command: ConfigCommand, paths: &ApplicationPaths) -> Response {
    let store = ConfigStore::new(paths.config_file());
    let value = match command {
        ConfigCommand::Show => store.read_protect_defaults().map_err(failure)?,
        ConfigCommand::Set(args) => {
            if let Some(id) = args.certificate_id.as_deref() {
                super::certificates::find(paths, id.trim())?;
            }
            let mut fields = Map::new();
            for (key, value) in [
                ("runtime_mode", args.runtime_mode),
                ("environment_policy", args.environment_policy),
                ("certificate_id", args.certificate_id),
                ("output_directory_mode", args.output_directory_mode),
                ("fixed_output_directory", args.fixed_output_directory),
            ] {
                if let Some(value) = value {
                    fields.insert(key.into(), json!(value));
                }
            }
            if let Some(value) = args.sign_after_protect {
                fields.insert("sign_after_protect".into(), json!(value));
            }
            if args.inherit_auto_sign {
                fields.insert("sign_after_protect".into(), Value::Null);
            }
            if args.clear_certificate {
                fields.insert("certificate_id".into(), Value::Null);
            }
            if fields.is_empty() {
                return Err(ManagementError::new(
                    "PATCH_REQUIRED",
                    "请明确指定需要保存的业务字段。",
                ));
            }
            let patch: ProtectDefaultsPatch = serde_json::from_value(Value::Object(fields))
                .map_err(|_| ManagementError::new("ARGUMENT_INVALID", "配置更新参数无效。"))?;
            store
                .update_protect_defaults(patch)
                .map_err(|error| failure(error).after_write())?
        }
    };
    Ok(json!({"protect_defaults": value}))
}
fn failure(error: shield_app::config::ConfigError) -> ManagementError {
    use shield_app::config::ConfigError;
    let code = match error {
        ConfigError::LockBusy => "CONFIG_BUSY",
        ConfigError::InvalidValue => "ARGUMENT_INVALID",
        ConfigError::UnsupportedVersion => "CONFIG_VERSION_UNSUPPORTED",
        _ => "CONFIG_OPERATION_FAILED",
    };
    ManagementError::new(
        code,
        "配置操作未完成，请检查配置格式、目录和锁状态；不会使用默认值覆盖原文件。",
    )
}

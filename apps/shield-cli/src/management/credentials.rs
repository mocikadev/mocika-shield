//! 新管理入口只接收约定的宿主环境，不从 argv、配置文件或模型输入取密码。
use super::ManagementError;

pub(super) struct Credentials {
    pub keystore: String,
    pub key: String,
}
pub(super) fn read() -> Result<Credentials, ManagementError> {
    let keystore = value("MOCIKA_SHIELD_KS_PASS")?.filter(|value| !value.trim().is_empty())
        .ok_or_else(|| ManagementError::new("CREDENTIAL_REQUIRED", "请在本机为当前命令预置 MOCIKA_SHIELD_KS_PASS；不要将密码放入命令参数或 Agent 对话。"))?;
    let key = value("MOCIKA_SHIELD_KEY_PASS")?.unwrap_or_default();
    Ok(Credentials { keystore, key })
}
fn value(name: &str) -> Result<Option<String>, ManagementError> {
    match std::env::var(name) {
        Ok(value) => Ok(Some(value)),
        Err(std::env::VarError::NotPresent) => Ok(None),
        Err(_) => Err(ManagementError::new(
            "CREDENTIAL_INVALID",
            "密码环境变量编码无效；不会输出原值。",
        )),
    }
}

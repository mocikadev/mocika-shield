//! 本地资料管理参数；不接受密码值，也不向受控执行器隐式授予权限。
use clap::{Args, Subcommand};
use std::path::PathBuf;

#[derive(Args)]
pub(crate) struct CertificateArgs {
    #[command(subcommand)]
    pub command: CertificateCommand,
}

#[derive(Subcommand)]
pub(crate) enum CertificateCommand {
    /// 查询不含密码的证书列表，不创建数据库。
    List,
    /// 按稳定编号查询摘要，不解密密码。
    Show { id: String },
    /// 导入已有证书；密码由专用环境变量提供，默认只记录原文件路径。
    Import(ImportArgs),
    /// 在应用托管目录创建证书，密码由专用环境变量提供。
    Create(CreateArgs),
    /// 重新校验已保存证书，并保存校验结果。
    Verify { id: String },
    /// 只更新指定偏好，不允许更改文件、Alias 或密码。
    Edit(EditArgs),
    /// 设置全局默认证书，不改加固页专用选择。
    SetDefault { id: String },
    /// 清除全局默认证书。
    ClearDefault,
    /// 删除记录但保留证书文件，必须显式确认。
    Remove {
        id: String,
        #[arg(long)]
        yes: bool,
    },
}

#[derive(Args, Default)]
pub(crate) struct SigningPreferences {
    #[arg(long)]
    pub v1: Option<bool>,
    #[arg(long)]
    pub v2: Option<bool>,
    #[arg(long)]
    pub v3: Option<bool>,
    #[arg(long)]
    pub v4: Option<bool>,
    #[arg(long)]
    pub auto_sign: Option<bool>,
}

#[derive(Args)]
pub(crate) struct ImportArgs {
    #[arg(long)]
    pub keystore: PathBuf,
    #[arg(long)]
    pub name: String,
    /// 留空时仅在证书恰有一个 Alias 的情况下自动选择。
    #[arg(long, default_value = "")]
    pub alias: String,
    #[arg(long, value_parser = ["jks", "pkcs12"], default_value = "jks")]
    pub keystore_type: String,
    #[arg(long, default_value = "")]
    pub note: String,
    #[arg(long)]
    pub set_default: bool,
    #[command(flatten)]
    pub signing: SigningPreferences,
}

#[derive(Args)]
pub(crate) struct CreateArgs {
    #[arg(long)]
    pub name: String,
    /// 只指定托管文件名，不接受目录。
    #[arg(long)]
    pub file_name: String,
    #[arg(long)]
    pub alias: String,
    #[arg(long)]
    pub dname: String,
    #[arg(long, value_parser = ["jks", "pkcs12"], default_value = "jks")]
    pub keystore_type: String,
    #[arg(long, default_value_t = 36500)]
    pub validity_days: u32,
    #[arg(long, default_value_t = 2048)]
    pub key_size: u32,
    #[arg(long, default_value = "")]
    pub note: String,
    #[arg(long)]
    pub set_default: bool,
    #[command(flatten)]
    pub signing: SigningPreferences,
}

#[derive(Args)]
pub(crate) struct EditArgs {
    pub id: String,
    #[arg(long)]
    pub name: Option<String>,
    #[arg(long)]
    pub note: Option<String>,
    #[command(flatten)]
    pub signing: SigningPreferences,
}

#[derive(Args)]
pub(crate) struct ConfigArgs {
    #[command(subcommand)]
    pub command: ConfigCommand,
}

#[derive(Subcommand)]
pub(crate) enum ConfigCommand {
    /// 只查询共享加固偏好，不返回统计或界面状态。
    Show,
    /// 持久化明确指定的字段，其余字段保持。
    Set(ConfigPatchArgs),
}

#[derive(Args)]
pub(crate) struct ConfigPatchArgs {
    #[arg(long, value_parser = ["standard", "android_api19"])]
    pub runtime_mode: Option<String>,
    #[arg(long, value_parser = ["compatible", "strict"])]
    pub environment_policy: Option<String>,
    #[arg(long, conflicts_with = "inherit_auto_sign")]
    pub sign_after_protect: Option<bool>,
    /// 清除明确的自动签名偏好，改为沿用默认证书偏好。
    #[arg(long)]
    pub inherit_auto_sign: bool,
    #[arg(long, conflicts_with = "clear_certificate")]
    pub certificate_id: Option<String>,
    /// 清除加固页专用证书选择，不删除证书。
    #[arg(long)]
    pub clear_certificate: bool,
    #[arg(long, value_parser = ["source", "fixed"])]
    pub output_directory_mode: Option<String>,
    #[arg(long)]
    pub fixed_output_directory: Option<String>,
}

//! 普通独立签名的共享身份适配；不改变受控 Agent 规划／执行协议。
use super::paths;
use crate::{
    args::SignArgs,
    config::{CliConfig, ResolvedSignArgs},
};
use anyhow::{anyhow, Result};
use shield_services::certificates::{
    resolve_signing_certificate, CertificateSelection, CertificateStoreState,
};
use std::path::Path;

pub(crate) fn resolve(
    config: &CliConfig,
    args: SignArgs,
    home: Option<&Path>,
) -> Result<ResolvedSignArgs> {
    // 在读取库之前拒绝混合材料；错误不能携带用户给出的密码或项目原文。
    config.require_shared_sign_source(&args)?;
    let paths = paths(home).map_err(|error| anyhow!(error.message))?;
    let store =
        CertificateStoreState::open_read_only(paths.certificate_database(), paths.keystore_dir())
            .map_err(|_| anyhow!("无法打开既有共享证书库；不会创建空库或切换到其他目录"))?;
    let selection = match args.certificate_id.as_deref() {
        Some(id) => CertificateSelection::Id(id),
        None => CertificateSelection::Default,
    };
    let certificate = resolve_signing_certificate(&store, selection).map_err(|_| {
        anyhow!("无法使用所选共享证书；请检查编号、默认项或本机密文状态，不会改用其他证书")
    })?;
    config.merge_shared_sign(args, certificate)
}

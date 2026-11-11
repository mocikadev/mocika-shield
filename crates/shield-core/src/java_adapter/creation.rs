//! 新建暂存证书的准备、原工具生成与核验编排；不保存资料或清理借用根。
use super::{
    certificates::{decode, invalid_result},
    *,
};
use crate::keytool::{CreateKeystoreOptions, CreatedKeystore};
use std::{fs, path::PathBuf};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Prepared {
    ascii_subject: String,
}

fn passwords(command: &mut Command, options: &CreateKeystoreOptions<'_>) {
    command
        .env(crate::java_process::KS_PASS_ENV, options.keystore_password)
        .env(
            crate::java_process::KEY_PASS_ENV,
            if options.key_password.trim().is_empty() {
                options.keystore_password
            } else {
                options.key_password
            },
        );
}

fn target(output: &Path) -> Result<PathBuf> {
    let output = crate::utils::strip_unc_prefix(&std::path::absolute(output)?);
    private_io::validate_directory(output.parent().ok_or_else(invalid_result)?)?;
    match fs::symlink_metadata(&output) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(output),
        _ => Err(invalid_result()),
    }
}

fn valid(options: &CreateKeystoreOptions<'_>) -> bool {
    [
        options.keystore_password,
        options.key_password,
        options.alias,
        options.dname,
    ]
    .iter()
    .all(|value| !value.contains('\0'))
        && options.keystore_password.trim().chars().count() >= 6
        && (options.key_password.trim().is_empty()
            || options.key_password.trim().chars().count() >= 6)
        && !options.alias.trim().is_empty()
        && options.alias.len() <= 4096
        && options.validity_days > 0
        && matches!(options.key_size, 2048 | 4096)
}

impl JavaToolContext {
    pub(crate) fn create_certificate(
        &self,
        java: &Path,
        keytool: &Path,
        output: &Path,
        options: &CreateKeystoreOptions<'_>,
    ) -> Result<CreatedKeystore> {
        if !valid(options) || !keytool.is_absolute() || !keytool.is_file() {
            return Err(invalid_result());
        }
        let output = target(output)?;
        let days = options.validity_days.to_string();
        let size = options.key_size.to_string();
        let common = [options.kind, options.alias, options.dname, &days, &size];
        self.workspace.verify()?;
        location::bootstrap_directory(self.workspace.path())?;
        let directory = private_io::directory(self.workspace.path())?;
        let bootstrap = location::bootstrap_directory(&directory)?;
        let raw = directory.join("generated.keystore");
        let raw_writer = private_io::create_file(&raw)?;
        let raw_identity = private_io::identity(&raw_writer)?;
        drop(raw_writer);
        let values = [
            "keystore_create_prepare",
            common[0],
            common[1],
            common[2],
            common[3],
            common[4],
            raw.to_str().ok_or_else(invalid_result)?,
        ];
        let mut command = self.prepare_request(java, &values)?;
        passwords(&mut command, options);
        let (status, bytes) = Self::collect(command, 65536)?;
        let prepared: Prepared = decode(&bytes, status.code(), values[0])
            .map_err(|error| error.context("新建证书主题准备失败"))?;
        if !prepared.ascii_subject.is_ascii()
            || prepared.ascii_subject.len() > 60000
            || prepared.ascii_subject.contains('\0')
        {
            return Err(invalid_result());
        }
        self.workspace.verify()?;
        private_io::validate_directory(&directory)?;
        let prepared_file = private_io::readonly(&raw)
            .map_err(|error| error.context("初始化证书材料访问核验失败"))?;
        if private_io::identity(&prepared_file)? != raw_identity
            || prepared_file.metadata()?.len() == 0
        {
            return Err(invalid_result());
        }
        drop(prepared_file);
        let mut command = crate::keytool::keytool_command(keytool);
        command
            .current_dir(bootstrap)
            .args([
                "-genkeypair",
                "-keystore",
                "generated.keystore",
                "-storetype",
                options.kind,
                "-storepass:env",
                crate::java_process::KS_PASS_ENV,
                "-keypass:env",
                crate::java_process::KEY_PASS_ENV,
                "-alias",
                "generated",
                "-dname",
                &prepared.ascii_subject,
                "-validity",
                &days,
                "-keyalg",
                "RSA",
                "-keysize",
                &size,
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        passwords(&mut command, options);
        let status = command.status()?;
        if !status.success() {
            let mut diagnostic = Diagnostic::new(FailureCode::ToolProcessFailed);
            diagnostic.tool = Some(crate::diagnostic::ToolName::Keytool);
            diagnostic.exit_code = status.code();
            return Err(diagnostic.attach(anyhow::anyhow!(
                "生成暂存证书的 keytool 进程失败；未保存证书或覆盖原件"
            )));
        }
        let raw_handle = private_io::readonly(&raw)
            .map_err(|error| error.context("密钥生成后材料访问核验失败"))?;
        if private_io::identity(&raw_handle)? != raw_identity {
            return Err(invalid_result());
        }
        let finished = directory.join("final.keystore");
        let final_writer = private_io::create_file(&finished)?;
        let final_identity = private_io::identity(&final_writer)?;
        drop(final_writer);
        let values = [
            "keystore_create_finish",
            common[0],
            common[1],
            common[2],
            common[3],
            common[4],
            raw.to_str().ok_or_else(invalid_result)?,
            finished.to_str().ok_or_else(invalid_result)?,
        ];
        let mut command = self.prepare_request(java, &values)?;
        passwords(&mut command, options);
        let (status, bytes) = Self::collect(command, 65536)?;
        let created: CreatedKeystore = decode(&bytes, status.code(), values[0])
            .map_err(|error| error.context("新建证书材料核验失败"))?;
        if created.alias.is_empty()
            || created.alias.len() > 4096
            || created.fingerprint.len() != 64
            || !created
                .fingerprint
                .bytes()
                .all(|ch| ch.is_ascii_digit() || (b'A'..=b'F').contains(&ch))
        {
            return Err(invalid_result());
        }
        self.workspace.verify()?;
        private_io::validate_directory(&directory)?;
        let mut source = private_io::readonly(&finished)
            .map_err(|error| error.context("整理结果访问核验失败"))?;
        if private_io::identity(&source)? != final_identity {
            return Err(invalid_result());
        }
        let length = source.metadata()?.len();
        if length == 0 || length > 1048576 {
            return Err(invalid_result());
        }
        let mut destination = private_io::create_file(&output)?;
        if std::io::copy(&mut (&mut source).take(1048577), &mut destination)? != length {
            return Err(invalid_result());
        }
        destination.sync_all()?;
        private_io::validate_file(&destination)?;
        Ok(created)
    }
}

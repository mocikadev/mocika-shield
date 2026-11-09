//! 普通单任务的临时产物编排；不实现或宣称 Agent 的进程隔离、取消和计划绑定。
use super::*;
use anyhow::{bail, Context};
use shield_core::{normalize_fingerprint, validate_runtime_resources, RuntimeProfile};
use std::{fs, io::ErrorKind, path::Path};

pub(super) fn run(mut args: ResolvedProtectArgs) -> Result<()> {
    // 沿用核心的 dunce 包装；尤其避免 canonicalize 后的 Windows 扩展路径传给 Java。
    args.input = shield_core::utils::strip_unc_prefix(&args.input);
    args.apktool = args
        .apktool
        .map(|path| shield_core::utils::strip_unc_prefix(&path));
    args.apksigner = args
        .apksigner
        .map(|path| shield_core::utils::strip_unc_prefix(&path));
    if let Some(signing) = args.signing.as_mut() {
        signing.keystore = shield_core::utils::strip_unc_prefix(&signing.keystore);
    }
    let target = output_target(&args.output)?;
    let profile = args.runtime_profile.context("缺少本次运行模式")?;
    let policy = match args.environment_policy {
        EnvironmentPolicyArg::Compatible => EnvironmentPolicy::Compatible,
        EnvironmentPolicyArg::Strict => EnvironmentPolicy::Strict,
    };
    let resources = resources(args.resources, profile)?;
    let signer = match args.apksigner {
        Some(path) => path,
        None => shield_core::utils::find_apksigner()?,
    };
    let checked = check_apk(&args.input, Some(&signer))?;
    profile.validate_native_abis(&checked.native_abis, &args.excluded_abis)?;
    let retained = checked
        .native_abis
        .into_iter()
        .filter(|abi| !args.excluded_abis.contains(abi))
        .collect::<Vec<_>>();
    validate_runtime_resources(&resources, profile, policy, &retained)?;
    let expected = args
        .signing
        .as_ref()
        .map(|certificate| {
            extract_keystore_cert_fingerprint(
                &certificate.keystore,
                &certificate.key_alias,
                &certificate.keystore_password,
                Some(match certificate.keystore_type {
                    KeystoreTypeArg::Jks => "JKS",
                    KeystoreTypeArg::Pkcs12 => "PKCS12",
                }),
            )
        })
        .transpose()?;
    let work = tempfile::Builder::new()
        .prefix(".mocika-protect-")
        .tempdir_in(target.parent().context("输出目录无效")?)?;
    let unsigned = work.path().join("protected.apk");
    let options = ProtectOptions {
        excluded_abis: args.excluded_abis,
        input: args.input,
        output: unsigned.clone(),
        apktool_path: args.apktool,
        resources_path: Some(resources),
        apksigner_path: Some(signer.clone()),
        expected_output_cert_fingerprint: expected.clone(),
        environment_policy: policy,
    };
    let json = args.json;
    protect_apk(
        &options,
        move |event| {
            if json {
                println!(
                    "{}",
                    progress_event_json(&format!("{:?}", event.step), &event.message)
                );
                let _ = std::io::stdout().flush();
            }
        },
        Arc::new(AtomicBool::new(false)),
    )?;
    let artifact = if let Some(certificate) = args.signing {
        let mut signing = sign_options(certificate);
        let signed = work.path().join("signed.apk");
        signing.apk_path = unsigned;
        signing.output_path = Some(signed.clone());
        signing.apksigner_path = Some(signer.clone());
        sign_apk_with_progress(&signing, |step| {
            if json {
                println!(
                    "{}",
                    progress_event_json(signing_step_name(step), signing_step_message(step))
                );
                let _ = std::io::stdout().flush();
            }
            Ok(())
        })?;
        let actual = extract_apk_cert_fingerprint(&signed, Some(&signer))?;
        if expected.as_deref().map(normalize_fingerprint) != Some(normalize_fingerprint(&actual)) {
            bail!("签名证书与加固时绑定的证书不一致，不交付最终文件");
        }
        signed
    } else {
        unsigned
    };
    if !check_apk(&artifact, Some(&signer))?.already_protected {
        bail!("暂存 APK 缺少加固标记，不交付最终文件");
    }
    publish(work, &artifact, &target)
}

fn resources(explicit: Option<PathBuf>, profile: RuntimeProfile) -> Result<PathBuf> {
    let path = match explicit {
        Some(path) => path,
        None => {
            let standard = shield_core::utils::find_runtime_resources()?;
            match profile {
                RuntimeProfile::Standard => standard,
                RuntimeProfile::AndroidApi19 => standard.with_file_name("resources-api19.zip"),
            }
        }
    };
    let path = shield_core::utils::strip_unc_prefix(&path);
    if !path.is_file() {
        bail!("所选模式的资源缺失，不会回退其他模式或资源包");
    }
    Ok(path)
}

fn output_target(output: &Path) -> Result<PathBuf> {
    if !output
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("apk"))
    {
        bail!("输出必须是明确的 APK 文件名");
    }
    let output = if output.is_absolute() {
        output.to_owned()
    } else {
        std::env::current_dir()?.join(output)
    };
    match fs::symlink_metadata(&output) {
        Err(error) if error.kind() == ErrorKind::NotFound => {}
        Ok(_) => bail!("输出已存在，不覆盖文件、目录或符号链接"),
        Err(_) => bail!("无法检查输出，不会尝试写入"),
    }
    let parent = output
        .parent()
        .context("输出目录无效")?
        .canonicalize()
        .context("输出父目录必须已存在")?;
    if !parent.is_dir() {
        bail!("输出父路径不是目录");
    }
    Ok(shield_core::utils::strip_unc_prefix(&parent)
        .join(output.file_name().context("输出文件名无效")?))
}

fn publish(work: tempfile::TempDir, artifact: &Path, target: &Path) -> Result<()> {
    // 同目录硬链接是单文件无覆盖提交；不回退复制，不承诺目录断电持久性或恶意进程沙箱。
    fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(artifact)?
        .sync_all()?;
    match fs::hard_link(artifact, target) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == ErrorKind::AlreadyExists => {
            bail!("输出已被其他操作占用，不覆盖已有文件")
        }
        Err(_) => {
            let _ = work.keep();
            bail!("最终输出交付未确认，已保留本次暂存材料；请先检查输出及同目录临时资料，不要自动重试");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn 提交不覆盖后来出现的文件且失败不残留最终输出() {
        let temp = tempfile::tempdir().unwrap();
        let target = output_target(&temp.path().join("output.apk")).unwrap();
        let work = tempfile::tempdir_in(temp.path()).unwrap();
        let artifact = work.path().join("signed.apk");
        fs::write(&artifact, b"fixture").unwrap();
        fs::write(&target, b"another").unwrap();
        assert!(publish(work, &artifact, &target).is_err());
        assert_eq!(fs::read(&target).unwrap(), b"another");
        assert!(!artifact.exists());
    }
    #[test]
    fn 单文件交付保留产物并清理本次暂存() {
        let temp = tempfile::tempdir().unwrap();
        let target = temp.path().join("output.apk");
        let work = tempfile::tempdir_in(temp.path()).unwrap();
        let artifact = work.path().join("signed.apk");
        fs::write(&artifact, b"fixture").unwrap();
        publish(work, &artifact, &target).unwrap();
        assert_eq!(fs::read(&target).unwrap(), b"fixture");
        assert!(!artifact.exists());
        assert!(output_target(&target).is_err());
    }
}

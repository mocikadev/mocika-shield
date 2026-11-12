use anyhow::Result;
use colored::Colorize;
use std::io::Write as _;
use std::path::PathBuf;
use std::sync::{atomic::AtomicBool, Arc};

use shield_core::utils::set_json_mode;
use shield_core::{
    check_apk, extract_apk_cert_fingerprint, extract_keystore_cert_fingerprint, protect_apk,
    sign_apk_with_progress, EnvironmentPolicy, KeystoreType, ProgressEvent, ProtectOptions,
    SignOptions, SigningProgressStep, SigningVersions,
};

use crate::args::{EnvironmentPolicyArg, KeystoreTypeArg};
use crate::config::{ResolvedProtectArgs, ResolvedSignArgs};

use crate::cli_json::{apk_check_json, done_event_json, keystore_check_json, progress_event_json};
mod protect_execution;

pub(crate) fn run_protect(args: ResolvedProtectArgs) -> Result<()> {
    if args.signing.is_some() && args.runtime_profile.is_none() {
        anyhow::bail!("签名任务缺少明确运行模式，不能退回旧的纯加固流程");
    }
    if args.json {
        set_json_mode(true);
    } else {
        let level = if args.verbose {
            log::LevelFilter::Debug
        } else {
            log::LevelFilter::Info
        };
        env_logger::Builder::from_default_env()
            .filter_level(level)
            .init();
    }

    if args.runtime_profile.is_some() {
        let json = args.json;
        protect_execution::run(args)?;
        if json {
            println!("{}", done_event_json());
        } else {
            println!("{}", "✓ 完成".green().bold());
        }
        return Ok(());
    }

    let opts = ProtectOptions {
        excluded_abis: args.excluded_abis,
        input: args.input,
        output: args.output,
        apktool_path: args.apktool,
        resources_path: args.resources,
        apksigner_path: args.apksigner,
        expected_output_cert_fingerprint: None,
        environment_policy: match args.environment_policy {
            EnvironmentPolicyArg::Compatible => EnvironmentPolicy::Compatible,
            EnvironmentPolicyArg::Strict => EnvironmentPolicy::Strict,
        },
    };

    let cancel = Arc::new(AtomicBool::new(false));
    let on_progress: Box<dyn Fn(ProgressEvent) + Send + 'static> = if args.json {
        Box::new(|event: ProgressEvent| {
            let step = format!("{:?}", event.step);
            println!("{}", progress_event_json(&step, &event.message));
            let _ = std::io::stdout().flush();
        })
    } else {
        Box::new(|_| {})
    };

    protect_apk(&opts, on_progress, cancel)?;
    if args.json {
        println!("{}", done_event_json());
    } else {
        println!("{}", "✓ 完成".green().bold());
    }
    Ok(())
}

pub(crate) fn run_sign(args: ResolvedSignArgs) -> Result<()> {
    let json = args.json;
    let options = sign_options(args);
    sign_apk_with_progress(&options, |step| {
        if json {
            println!(
                "{}",
                progress_event_json(signing_step_name(step), signing_step_message(step))
            );
            let _ = std::io::stdout().flush();
        }
        Ok(())
    })?;
    if json {
        println!("{}", done_event_json());
    } else {
        println!("{}", "✓ 签名完成".green().bold());
    }
    Ok(())
}

fn sign_options(args: ResolvedSignArgs) -> SignOptions {
    SignOptions {
        apk_path: args.input,
        output_path: Some(args.output),
        keystore_path: args.keystore,
        key_alias: args.key_alias,
        keystore_password: args.keystore_password,
        key_password: args.key_password,
        apksigner_path: args.apksigner,
        keystore_type: match args.keystore_type {
            KeystoreTypeArg::Jks => KeystoreType::Jks,
            KeystoreTypeArg::Pkcs12 => KeystoreType::Pkcs12,
        },
        signing_versions: SigningVersions {
            v1: args.v1,
            v2: args.v2,
            v3: args.v3,
            v4: args.v4,
        },
    }
}

fn signing_step_name(step: SigningProgressStep) -> &'static str {
    match step {
        SigningProgressStep::Prepare => "prepare",
        SigningProgressStep::Align => "align",
        SigningProgressStep::Sign => "sign",
        SigningProgressStep::Commit => "sign",
    }
}

fn signing_step_message(step: SigningProgressStep) -> &'static str {
    match step {
        SigningProgressStep::Prepare => "准备签名环境",
        SigningProgressStep::Align => "对齐 APK",
        SigningProgressStep::Sign => "写入 APK 签名",
        SigningProgressStep::Commit => "保存签名产物",
    }
}

pub(crate) fn run_check_apk(path: PathBuf) -> Result<String> {
    match check_apk(&path, None) {
        Ok(result) => {
            let cert_fingerprint = if result.is_signed {
                extract_apk_cert_fingerprint(&path, None).ok()
            } else {
                None
            };
            Ok(apk_check_json(
                result.already_protected,
                result.is_signed,
                cert_fingerprint,
            ))
        }
        Err(err) => Err(err),
    }
}

pub(crate) fn run_check_keystore(ks: PathBuf, alias: String, ks_pass: String) -> Result<String> {
    match extract_keystore_cert_fingerprint(&ks, &alias, &ks_pass, None) {
        Ok(fp) => Ok(keystore_check_json(fp)),
        Err(err) => Err(err),
    }
}

use anyhow::Result;
use colored::Colorize;
use std::io::Write as _;
use std::path::PathBuf;
use std::sync::{atomic::AtomicBool, Arc};

use shield_core::utils::set_json_mode;
use shield_core::{
    check_apk, extract_apk_cert_fingerprint, extract_keystore_cert_fingerprint, protect_apk,
    ProgressEvent, ProtectOptions,
};

use crate::cli_json::{
    apk_check_error_json, apk_check_json, done_event_json, keystore_check_error_json,
    keystore_check_json, progress_event_json,
};

pub(crate) fn run_protect(
    input: PathBuf,
    output: PathBuf,
    json_progress: bool,
    verbose: bool,
) -> Result<()> {
    if json_progress {
        set_json_mode(true);
    } else {
        let level = if verbose {
            log::LevelFilter::Debug
        } else {
            log::LevelFilter::Info
        };
        env_logger::Builder::from_default_env()
            .filter_level(level)
            .init();
    }

    let opts = ProtectOptions {
        input,
        output,
        apktool_path: None,
        resources_path: None,
        apksigner_path: None,
        expected_output_cert_fingerprint: None,
    };

    let cancel = Arc::new(AtomicBool::new(false));
    let on_progress: Box<dyn Fn(ProgressEvent) + Send + 'static> = if json_progress {
        Box::new(|event: ProgressEvent| {
            let step = format!("{:?}", event.step);
            println!("{}", progress_event_json(&step, &event.message));
            let _ = std::io::stdout().flush();
        })
    } else {
        Box::new(|_| {})
    };

    match protect_apk(&opts, on_progress, cancel) {
        Ok(()) => {
            if json_progress {
                println!("{}", done_event_json());
            } else {
                println!("{}", "✓ 完成".green().bold());
            }
            Ok(())
        }
        Err(err) => {
            eprintln!("{err}");
            std::process::exit(1);
        }
    }
}

pub(crate) fn run_check_apk(path: PathBuf) -> String {
    match check_apk(&path, None) {
        Ok(result) => {
            let cert_fingerprint = if result.is_signed {
                extract_apk_cert_fingerprint(&path, None).ok()
            } else {
                None
            };
            apk_check_json(result.already_protected, result.is_signed, cert_fingerprint)
        }
        Err(err) => apk_check_error_json(err.to_string()),
    }
}

pub(crate) fn run_check_keystore(ks: PathBuf, alias: String, ks_pass: String) -> String {
    match extract_keystore_cert_fingerprint(&ks, &alias, &ks_pass, None) {
        Ok(fp) => keystore_check_json(fp),
        Err(err) => keystore_check_error_json(err.to_string()),
    }
}

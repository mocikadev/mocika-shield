use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use colored::Colorize;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::{atomic::AtomicBool, Arc};

use shield_core::utils::set_json_mode;
use shield_core::{protect_apk, ProgressEvent, ProtectOptions};

#[derive(Parser)]
#[command(
    name = "shield",
    version,
    author,
    about = "Android APK 加固工具",
    arg_required_else_help = true
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    Protect {
        #[arg(short, long, value_name = "APK")]
        input: PathBuf,
        #[arg(short, long, value_name = "APK")]
        output: PathBuf,
        #[arg(long)]
        json_progress: bool,
        #[arg(short, long)]
        verbose: bool,
    },
    CheckApk {
        path: PathBuf,
    },
    CheckKeystore {
        #[arg(long)]
        ks: PathBuf,
        #[arg(long)]
        alias: String,
        #[arg(long)]
        ks_pass: String,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Protect {
            input,
            output,
            json_progress,
            verbose,
        } => {
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
            };

            let cancel = Arc::new(AtomicBool::new(false));

            let on_progress: Box<dyn Fn(ProgressEvent) + Send + 'static> = if json_progress {
                Box::new(|event: ProgressEvent| {
                    let step = format!("{:?}", event.step);
                    let msg = json_escape(&event.message);
                    println!("{{\"type\":\"progress\",\"step\":\"{step}\",\"message\":\"{msg}\"}}");
                    let _ = std::io::stdout().flush();
                })
            } else {
                Box::new(|_| {})
            };

            match protect_apk(&opts, on_progress, cancel) {
                Ok(()) => {
                    if json_progress {
                        println!("{{\"type\":\"done\"}}");
                    } else {
                        println!("{}", "✓ 完成".green().bold());
                    }
                }
                Err(e) => {
                    eprintln!("{}", e);
                    std::process::exit(1);
                }
            }
        }

        Commands::CheckApk { path } => {
            let result = check_apk_json(&path);
            println!("{result}");
        }

        Commands::CheckKeystore { ks, alias, ks_pass } => {
            let result = check_keystore_json(&ks, &alias, &ks_pass);
            println!("{result}");
        }
    }

    Ok(())
}

fn has_v2_v3_signature(apk_path: &Path) -> bool {
    use std::io::{Read, Seek, SeekFrom};
    const MAGIC: &[u8] = b"APK Sig Block 42";
    let Ok(mut f) = std::fs::File::open(apk_path) else {
        return false;
    };
    let Ok(meta) = f.metadata() else { return false };
    let file_size = meta.len();
    let scan_size = file_size.min(65536) as usize;
    let offset = file_size.saturating_sub(scan_size as u64);
    if f.seek(SeekFrom::Start(offset)).is_err() {
        return false;
    }
    let mut buf = vec![0u8; scan_size];
    let n = f.read(&mut buf).unwrap_or(0);
    buf.truncate(n);
    buf.windows(MAGIC.len()).any(|w| w == MAGIC)
}

fn check_apk_json(path: &Path) -> String {
    use std::io::Read;

    let Ok(file) = std::fs::File::open(path) else {
        return "{\"error\":\"无法打开文件\"}".to_string();
    };
    let Ok(mut archive) = zip::ZipArchive::new(file) else {
        return "{\"error\":\"无法解析 APK\"}".to_string();
    };

    let mut already_protected = false;
    let mut is_signed = false;
    const MSHD: &[u8] = b"MSHD";
    const TAIL: u64 = 4096;

    for i in 0..archive.len() {
        let mut entry = match archive.by_index(i) {
            Ok(e) => e,
            Err(_) => continue,
        };
        let name = entry.name().to_owned();

        if name == "classes.dex" && !already_protected {
            let size = entry.size();
            if size >= 8 {
                let skip = size.saturating_sub(TAIL);
                let read_len = (size - skip) as usize;
                let mut tail = vec![0u8; read_len];
                let ok = (skip == 0
                    || std::io::copy(&mut entry.by_ref().take(skip), &mut std::io::sink()).is_ok())
                    && entry.read_exact(&mut tail).is_ok();
                if ok {
                    already_protected = tail.windows(MSHD.len()).any(|w| w == MSHD);
                }
            }
        }

        if name.starts_with("META-INF/")
            && (name.ends_with(".RSA") || name.ends_with(".DSA") || name.ends_with(".EC"))
        {
            is_signed = true;
        }

        if already_protected && is_signed {
            break;
        }
    }

    if !is_signed {
        is_signed = has_v2_v3_signature(path);
    }

    let cert_fingerprint = if is_signed {
        extract_apk_cert_fingerprint(path).unwrap_or_default()
    } else {
        String::new()
    };

    let fp_json = if cert_fingerprint.is_empty() {
        "null".to_string()
    } else {
        format!("\"{}\"", json_escape(&cert_fingerprint))
    };

    format!(
        "{{\"already_protected\":{already_protected},\"is_signed\":{is_signed},\"cert_fingerprint\":{fp_json}}}"
    )
}

fn check_keystore_json(ks: &Path, alias: &str, ks_pass: &str) -> String {
    match extract_keystore_cert_fingerprint(ks, alias, ks_pass) {
        Ok(fp) => format!("{{\"cert_fingerprint\":\"{}\"}}", json_escape(&fp)),
        Err(e) => format!("{{\"error\":\"{}\"}}", json_escape(&e.to_string())),
    }
}

fn extract_apk_cert_fingerprint(apk_path: &Path) -> Result<String> {
    if let Ok(keytool) = shield_core::utils::find_keytool() {
        let v1 = std::process::Command::new(&keytool)
            .args(["-printcert", "-jarfile", apk_path.to_str().unwrap_or("")])
            .output();
        if let Ok(out) = v1 {
            if out.status.success() {
                let stdout = String::from_utf8_lossy(&out.stdout);
                if let Some(fp) = parse_sha256_from_keytool(&stdout) {
                    return Ok(fp);
                }
            }
        }
    }

    let apksigner =
        shield_core::utils::find_apksigner().context("V1 证书提取失败，且未找到 apksigner.jar")?;
    let java = shield_core::utils::find_java().context("V1 证书提取失败，且未找到 Java")?;
    let out2 = std::process::Command::new(&java)
        .args([
            "-jar",
            apksigner.to_str().unwrap_or(""),
            "verify",
            "--print-certs",
            apk_path.to_str().unwrap_or(""),
        ])
        .output()
        .context("执行 apksigner verify 失败")?;
    if out2.status.success() {
        let stdout = String::from_utf8_lossy(&out2.stdout);
        if let Some(fp) = parse_sha256_from_apksigner(&stdout) {
            return Ok(fp);
        }
    }
    anyhow::bail!("无法提取 APK 证书指纹（V1 和 V2/V3 均失败）")
}

fn parse_sha256_from_apksigner(output: &str) -> Option<String> {
    for line in output.lines() {
        let upper = line.trim().to_uppercase();
        if upper.contains("SHA-256") && upper.contains("DIGEST") {
            if let Some(pos) = line.rfind(':') {
                let hex = line[pos + 1..].trim().to_uppercase();
                if hex.len() == 64 && hex.chars().all(|c| c.is_ascii_hexdigit()) {
                    return Some(hex);
                }
            }
        }
    }
    None
}

fn extract_keystore_cert_fingerprint(ks: &Path, alias: &str, ks_pass: &str) -> Result<String> {
    let keytool = shield_core::utils::find_keytool()?;
    let output = std::process::Command::new(&keytool)
        .args([
            "-list",
            "-keystore",
            ks.to_str().unwrap_or(""),
            "-alias",
            alias,
            "-storepass",
            ks_pass,
            "-v",
        ])
        .output()?;

    if output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        if let Some(fp) = parse_sha256_from_keytool(&stdout) {
            return Ok(fp);
        }
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    anyhow::bail!("无法读取 keystore 证书指纹：{}", stderr.trim())
}

fn parse_sha256_from_keytool(output: &str) -> Option<String> {
    for line in output.lines() {
        let trimmed = line.trim();
        if trimmed.to_uppercase().contains("SHA256") && trimmed.contains(':') {
            let after_label = trimmed.splitn(2, ':').nth(1)?;
            let clean = after_label.replace(':', "").replace(' ', "").to_uppercase();
            if clean.len() == 64 && clean.chars().all(|c| c.is_ascii_hexdigit()) {
                return Some(clean);
            }
        }
    }
    None
}

fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c => out.push(c),
        }
    }
    out
}

use crate::{check_apk, extract_apk_cert_fingerprint, normalize_fingerprint};
use anyhow::{Context, Result};
use std::fs;
use std::path::Path;

const STANDARD_ABIS: &[&str] = &["armeabi-v7a", "arm64-v8a", "x86", "x86_64"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreflightSeverity {
    Ready,
    Warning,
    Blocked,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeProfile {
    Standard,
    AndroidApi19,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreflightCheck {
    pub code: &'static str,
    pub severity: PreflightSeverity,
    pub detail: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreflightFacts {
    pub apk_size: u64,
    pub dex_count: u32,
    pub dex_total_size: u64,
    pub native_library_count: u32,
    pub compressed_native_library_count: u32,
    pub native_abis: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreflightReport {
    pub verdict: PreflightSeverity,
    pub checks: Vec<PreflightCheck>,
    pub facts: PreflightFacts,
}

pub struct PreflightOptions<'a> {
    pub runtime_profile: RuntimeProfile,
    pub apksigner_path: Option<&'a Path>,
    pub expected_output_cert_fingerprint: Option<&'a str>,
}

#[derive(Debug)]
struct ArchiveFacts {
    has_manifest: bool,
    apk_size: u64,
    dex_count: u32,
    dex_total_size: u64,
    native_library_count: u32,
    compressed_native_library_count: u32,
}

pub fn preflight_apk(path: &Path, options: PreflightOptions<'_>) -> Result<PreflightReport> {
    let outcome = check_apk(path, options.apksigner_path)?;
    let archive = inspect_archive(path)?;
    let mut checks = evaluate_archive(
        &archive,
        &outcome.native_abis,
        outcome.already_protected,
        outcome.is_signed,
        options.runtime_profile,
    );

    if outcome.is_signed {
        if let Some(expected) = options.expected_output_cert_fingerprint {
            match extract_apk_cert_fingerprint(path, options.apksigner_path) {
                Ok(actual) if normalize_fingerprint(&actual) == normalize_fingerprint(expected) => {
                    checks.push(check("certificate", PreflightSeverity::Ready, None));
                }
                Ok(_) => checks.push(check(
                    "certificate_mismatch",
                    PreflightSeverity::Blocked,
                    None,
                )),
                Err(error) => checks.push(check(
                    "certificate_unreadable",
                    PreflightSeverity::Blocked,
                    Some(error.to_string()),
                )),
            }
        }
    }

    let verdict = checks
        .iter()
        .map(|item| item.severity)
        .max_by_key(|severity| severity_rank(*severity))
        .unwrap_or(PreflightSeverity::Ready);

    Ok(PreflightReport {
        verdict,
        checks,
        facts: PreflightFacts {
            apk_size: archive.apk_size,
            dex_count: archive.dex_count,
            dex_total_size: archive.dex_total_size,
            native_library_count: archive.native_library_count,
            compressed_native_library_count: archive.compressed_native_library_count,
            native_abis: outcome.native_abis,
        },
    })
}

fn inspect_archive(path: &Path) -> Result<ArchiveFacts> {
    let apk_size = fs::metadata(path).context("读取 APK 文件大小失败")?.len();
    let file = fs::File::open(path).context("打开 APK 失败")?;
    let mut zip = zip::ZipArchive::new(file).context("解析 APK ZIP 结构失败")?;
    let mut facts = ArchiveFacts {
        has_manifest: false,
        apk_size,
        dex_count: 0,
        dex_total_size: 0,
        native_library_count: 0,
        compressed_native_library_count: 0,
    };

    for index in 0..zip.len() {
        let entry = zip.by_index(index).context("读取 APK ZIP 条目失败")?;
        let name = entry.name();
        if name == "AndroidManifest.xml" {
            facts.has_manifest = true;
        }
        if is_dex_entry(name) {
            facts.dex_count += 1;
            facts.dex_total_size = facts.dex_total_size.saturating_add(entry.size());
        }
        if is_native_library(name) {
            facts.native_library_count += 1;
            if entry.compression() != zip::CompressionMethod::Stored {
                facts.compressed_native_library_count += 1;
            }
        }
    }
    Ok(facts)
}

fn evaluate_archive(
    archive: &ArchiveFacts,
    native_abis: &[String],
    already_protected: bool,
    is_signed: bool,
    runtime_profile: RuntimeProfile,
) -> Vec<PreflightCheck> {
    let mut checks = Vec::new();
    if !archive.has_manifest || archive.dex_count == 0 {
        let mut missing = Vec::new();
        if !archive.has_manifest {
            missing.push("AndroidManifest.xml");
        }
        if archive.dex_count == 0 {
            missing.push("classes.dex");
        }
        checks.push(check(
            "apk_structure",
            PreflightSeverity::Blocked,
            Some(missing.join("、")),
        ));
    } else {
        checks.push(check("apk_structure", PreflightSeverity::Ready, None));
    }

    checks.push(if already_protected {
        check("already_protected", PreflightSeverity::Blocked, None)
    } else {
        check("not_protected", PreflightSeverity::Ready, None)
    });
    checks.push(if is_signed {
        check("signature", PreflightSeverity::Ready, None)
    } else {
        check("unsigned", PreflightSeverity::Blocked, None)
    });

    checks.push(check(
        "dex_profile",
        PreflightSeverity::Ready,
        Some(format!("{}|{}", archive.dex_count, archive.dex_total_size)),
    ));

    let unsupported: Vec<String> = match runtime_profile {
        RuntimeProfile::AndroidApi19 => native_abis
            .iter()
            .filter(|abi| abi.as_str() != "armeabi-v7a")
            .cloned()
            .collect(),
        RuntimeProfile::Standard => native_abis
            .iter()
            .filter(|abi| !STANDARD_ABIS.contains(&abi.as_str()))
            .cloned()
            .collect(),
    };
    let supported_count = native_abis.len().saturating_sub(unsupported.len());
    let abi_severity = if unsupported.is_empty() {
        PreflightSeverity::Ready
    } else if runtime_profile == RuntimeProfile::AndroidApi19 || supported_count == 0 {
        PreflightSeverity::Blocked
    } else {
        PreflightSeverity::Warning
    };
    checks.push(check(
        "runtime_abi",
        abi_severity,
        (!unsupported.is_empty()).then(|| unsupported.join("、")),
    ));

    checks.push(check(
        "native_packaging",
        PreflightSeverity::Ready,
        Some(format!(
            "{}|{}",
            archive.native_library_count, archive.compressed_native_library_count
        )),
    ));
    checks
}

fn check(
    code: &'static str,
    severity: PreflightSeverity,
    detail: Option<String>,
) -> PreflightCheck {
    PreflightCheck {
        code,
        severity,
        detail,
    }
}

fn severity_rank(severity: PreflightSeverity) -> u8 {
    match severity {
        PreflightSeverity::Ready => 0,
        PreflightSeverity::Warning => 1,
        PreflightSeverity::Blocked => 2,
    }
}

fn is_dex_entry(name: &str) -> bool {
    let Some(stem) = name.strip_suffix(".dex") else {
        return false;
    };
    stem == "classes"
        || stem
            .strip_prefix("classes")
            .is_some_and(|suffix| !suffix.is_empty() && suffix.chars().all(|c| c.is_ascii_digit()))
}

fn is_native_library(name: &str) -> bool {
    name.starts_with("lib/") && name.ends_with(".so") && name.split('/').count() >= 3
}

#[cfg(test)]
mod tests {
    use super::*;

    fn archive() -> ArchiveFacts {
        ArchiveFacts {
            has_manifest: true,
            apk_size: 1024,
            dex_count: 2,
            dex_total_size: 800,
            native_library_count: 1,
            compressed_native_library_count: 0,
        }
    }

    #[test]
    fn 标准运行时支持四种主流架构() {
        let checks = evaluate_archive(
            &archive(),
            &["arm64-v8a".to_string(), "x86_64".to_string()],
            false,
            true,
            RuntimeProfile::Standard,
        );
        assert_eq!(
            checks
                .iter()
                .find(|item| item.code == "runtime_abi")
                .unwrap()
                .severity,
            PreflightSeverity::Ready
        );
    }

    #[test]
    fn 安卓四点四模式阻止非_v7a_架构() {
        let checks = evaluate_archive(
            &archive(),
            &["armeabi-v7a".to_string(), "arm64-v8a".to_string()],
            false,
            true,
            RuntimeProfile::AndroidApi19,
        );
        assert_eq!(
            checks
                .iter()
                .find(|item| item.code == "runtime_abi")
                .unwrap()
                .severity,
            PreflightSeverity::Blocked
        );
    }

    #[test]
    fn 缺少清单或_dex_时必须阻止() {
        let mut facts = archive();
        facts.has_manifest = false;
        facts.dex_count = 0;
        let checks = evaluate_archive(&facts, &[], false, true, RuntimeProfile::Standard);
        assert_eq!(checks[0].severity, PreflightSeverity::Blocked);
        assert_eq!(
            checks[0].detail.as_deref(),
            Some("AndroidManifest.xml、classes.dex")
        );
    }

    #[test]
    fn 未知架构与支持架构并存时给出风险() {
        let checks = evaluate_archive(
            &archive(),
            &["arm64-v8a".to_string(), "mips".to_string()],
            false,
            true,
            RuntimeProfile::Standard,
        );
        assert_eq!(
            checks
                .iter()
                .find(|item| item.code == "runtime_abi")
                .unwrap()
                .severity,
            PreflightSeverity::Warning
        );
    }

    #[test]
    fn 仅未知架构时必须阻止() {
        let checks = evaluate_archive(
            &archive(),
            &["armeabi".to_string()],
            false,
            true,
            RuntimeProfile::Standard,
        );
        assert_eq!(
            checks
                .iter()
                .find(|item| item.code == "runtime_abi")
                .unwrap()
                .severity,
            PreflightSeverity::Blocked
        );
    }

    #[test]
    fn 只识别根目录标准多_dex_名称() {
        assert!(is_dex_entry("classes.dex"));
        assert!(is_dex_entry("classes2.dex"));
        assert!(!is_dex_entry("assets/classes.dex"));
        assert!(!is_dex_entry("classesx.dex"));
    }
}

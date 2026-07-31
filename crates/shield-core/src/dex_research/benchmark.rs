//! 仅用于本地研究的真实多 DEX 封装资源成本编排。

use std::{
    fmt, fs, io,
    path::{Path, PathBuf},
    process::Command,
    time::{Duration, Instant},
};

use thiserror::Error;

use super::{
    bundle::{open_bundle, seal_bundle, BundleCarrier, BundleError},
    layered::{open_layered_each, seal_layered, LayeredError},
    manifest::GroupMember,
    transform::{extract_and_stub, Extraction, TransformError},
};

#[derive(Debug, Error)]
pub(super) enum BenchmarkError {
    #[error("读取 DEX 样本失败：{0}")]
    Io(#[from] io::Error),
    #[error("目录中没有 classes*.dex 样本")]
    NoDexFiles,
    #[error("转换 DEX 失败：{0}")]
    Transform(#[from] TransformError),
    #[error("封装 DEX 失败：{0}")]
    Bundle(#[from] BundleError),
    #[error("恢复结果数量不匹配：expected={expected}, actual={actual}")]
    RestoredCountMismatch { expected: usize, actual: usize },
    #[error("DEX {identity} 恢复结果不一致")]
    RestoredContentMismatch { identity: String },
    #[error("分层认证索引验证失败：{0}")]
    Layered(#[from] LayeredError),
}

#[derive(Debug)]
pub(super) struct LayeredReport {
    dex_count: usize,
    index_ciphertext_bytes: usize,
    shard_ciphertext_bytes: usize,
}

#[derive(Debug)]
pub(super) struct BenchmarkReport {
    strategy: &'static str,
    dex_count: usize,
    original_bytes: usize,
    carrier_bytes: usize,
    extracted_instruction_bytes: usize,
    stub_instruction_bytes: usize,
    ciphertext_bytes: usize,
    read_duration: Duration,
    extract_duration: Duration,
    seal_duration: Duration,
    open_duration: Duration,
    rss_after_read_kib: Option<u64>,
    rss_after_extract_kib: Option<u64>,
    rss_after_seal_kib: Option<u64>,
    rss_after_open_kib: Option<u64>,
}

pub(super) fn run(input_dir: &Path) -> Result<BenchmarkReport, BenchmarkError> {
    let paths = dex_paths(input_dir)?;
    let read_started = Instant::now();
    let originals = paths.iter().map(fs::read).collect::<Result<Vec<_>, _>>()?;
    let read_duration = read_started.elapsed();
    let rss_after_read_kib = current_rss_kib();

    let extract_started = Instant::now();
    let extractions = originals
        .iter()
        .map(|dex| extract_and_stub(dex))
        .collect::<Result<Vec<_>, _>>()?;
    let extract_duration = extract_started.elapsed();
    let rss_after_extract_kib = current_rss_kib();

    let identities = paths
        .iter()
        .map(|path| path.file_name().unwrap().to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    let members = group_members(&identities, &originals, &extractions);
    let key = [0x6du8; 32];
    let nonce = [0x27u8; 12];
    let seal_started = Instant::now();
    let sealed = seal_bundle(&key, nonce, &members)?;
    let seal_duration = seal_started.elapsed();
    let rss_after_seal_kib = current_rss_kib();

    let carriers = identities
        .iter()
        .zip(&extractions)
        .map(|(identity, extraction)| BundleCarrier::new(identity, &extraction.carrier))
        .collect::<Vec<_>>();
    let open_started = Instant::now();
    let restored = open_bundle(&key, &sealed, &carriers)?;
    let open_duration = open_started.elapsed();
    let rss_after_open_kib = current_rss_kib();
    if restored.len() != originals.len() {
        return Err(BenchmarkError::RestoredCountMismatch {
            expected: originals.len(),
            actual: restored.len(),
        });
    }
    for ((identity, original), actual) in identities.iter().zip(&originals).zip(&restored) {
        if actual != original {
            return Err(BenchmarkError::RestoredContentMismatch {
                identity: identity.clone(),
            });
        }
    }

    Ok(BenchmarkReport {
        strategy: "整组封装",
        dex_count: originals.len(),
        original_bytes: originals.iter().map(Vec::len).sum(),
        carrier_bytes: extractions
            .iter()
            .map(|extraction| extraction.carrier.len())
            .sum(),
        extracted_instruction_bytes: instruction_bytes(&extractions, |method| {
            method.original.len()
        }),
        stub_instruction_bytes: instruction_bytes(&extractions, |method| method.stub.len()),
        ciphertext_bytes: sealed.ciphertext.len(),
        read_duration,
        extract_duration,
        seal_duration,
        open_duration,
        rss_after_read_kib,
        rss_after_extract_kib,
        rss_after_seal_kib,
        rss_after_open_kib,
    })
}

pub(super) fn run_per_dex(input_dir: &Path) -> Result<BenchmarkReport, BenchmarkError> {
    let paths = dex_paths(input_dir)?;
    let key = [0x6du8; 32];
    let mut report = BenchmarkReport {
        strategy: "逐 DEX 独立封装",
        dex_count: paths.len(),
        original_bytes: 0,
        carrier_bytes: 0,
        extracted_instruction_bytes: 0,
        stub_instruction_bytes: 0,
        ciphertext_bytes: 0,
        read_duration: Duration::ZERO,
        extract_duration: Duration::ZERO,
        seal_duration: Duration::ZERO,
        open_duration: Duration::ZERO,
        rss_after_read_kib: None,
        rss_after_extract_kib: None,
        rss_after_seal_kib: None,
        rss_after_open_kib: None,
    };

    for (index, path) in paths.iter().enumerate() {
        let identity = path.file_name().unwrap().to_string_lossy().into_owned();
        let started = Instant::now();
        let original = fs::read(path)?;
        report.read_duration += started.elapsed();
        report.original_bytes += original.len();
        update_max_rss(&mut report.rss_after_read_kib);

        let started = Instant::now();
        let extraction = extract_and_stub(&original)?;
        report.extract_duration += started.elapsed();
        report.carrier_bytes += extraction.carrier.len();
        report.extracted_instruction_bytes += extraction
            .payload
            .methods
            .iter()
            .map(|method| method.original.len())
            .sum::<usize>();
        report.stub_instruction_bytes += extraction
            .payload
            .methods
            .iter()
            .map(|method| method.stub.len())
            .sum::<usize>();
        update_max_rss(&mut report.rss_after_extract_kib);

        let member = [GroupMember::new(&identity, &original, &extraction)];
        let nonce = nonce_for_index(index);
        let started = Instant::now();
        let sealed = seal_bundle(&key, nonce, &member)?;
        report.seal_duration += started.elapsed();
        report.ciphertext_bytes += sealed.ciphertext.len();
        update_max_rss(&mut report.rss_after_seal_kib);

        let carrier = [BundleCarrier::new(&identity, &extraction.carrier)];
        let started = Instant::now();
        let restored = open_bundle(&key, &sealed, &carrier)?;
        report.open_duration += started.elapsed();
        update_max_rss(&mut report.rss_after_open_kib);
        if restored.len() != 1 {
            return Err(BenchmarkError::RestoredCountMismatch {
                expected: 1,
                actual: restored.len(),
            });
        }
        if restored[0] != original {
            return Err(BenchmarkError::RestoredContentMismatch { identity });
        }
    }
    Ok(report)
}

pub(super) fn verify_layered(input_dir: &Path) -> Result<LayeredReport, BenchmarkError> {
    let paths = dex_paths(input_dir)?;
    let originals = paths.iter().map(fs::read).collect::<Result<Vec<_>, _>>()?;
    let extractions = originals
        .iter()
        .map(|dex| extract_and_stub(dex))
        .collect::<Result<Vec<_>, _>>()?;
    let identities = paths
        .iter()
        .map(|path| path.file_name().unwrap().to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    let members = group_members(&identities, &originals, &extractions);
    let key = [0x38u8; 32];
    let package = seal_layered(&key, [0x49u8; 12], &members)?;
    let carriers = identities
        .iter()
        .zip(&extractions)
        .map(|(identity, extraction)| BundleCarrier::new(identity, &extraction.carrier))
        .collect::<Vec<_>>();
    let mut restored_index = 0usize;
    open_layered_each(&key, &package, &carriers, |identity, dex| {
        let expected_identity = &identities[restored_index];
        if identity != expected_identity {
            return Err(format!(
                "DEX 身份顺序不一致：expected={expected_identity}, actual={identity}"
            ));
        }
        if dex != originals[restored_index] {
            return Err(format!("DEX {identity} 恢复内容不一致"));
        }
        restored_index += 1;
        Ok(())
    })?;
    if restored_index != originals.len() {
        return Err(BenchmarkError::RestoredCountMismatch {
            expected: originals.len(),
            actual: restored_index,
        });
    }
    Ok(LayeredReport {
        dex_count: originals.len(),
        index_ciphertext_bytes: package.index_ciphertext.len(),
        shard_ciphertext_bytes: package
            .shards
            .iter()
            .map(|shard| shard.ciphertext.len())
            .sum(),
    })
}

fn dex_paths(input_dir: &Path) -> Result<Vec<PathBuf>, BenchmarkError> {
    let mut paths = fs::read_dir(input_dir)?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<Result<Vec<_>, _>>()?;
    paths.retain(|path| dex_sequence(path).is_some());
    paths.sort_by_key(|path| dex_sequence(path).unwrap());
    if paths.is_empty() {
        return Err(BenchmarkError::NoDexFiles);
    }
    Ok(paths)
}

fn dex_sequence(path: &Path) -> Option<u32> {
    let name = path.file_name()?.to_str()?;
    if name == "classes.dex" {
        return Some(1);
    }
    name.strip_prefix("classes")?
        .strip_suffix(".dex")?
        .parse()
        .ok()
}

fn group_members<'a>(
    identities: &'a [String],
    originals: &'a [Vec<u8>],
    extractions: &'a [Extraction],
) -> Vec<GroupMember<'a>> {
    identities
        .iter()
        .zip(originals)
        .zip(extractions)
        .map(|((identity, original), extraction)| GroupMember::new(identity, original, extraction))
        .collect()
}

fn instruction_bytes(
    extractions: &[Extraction],
    length: impl Fn(&super::transform::ExtractedMethod) -> usize,
) -> usize {
    extractions
        .iter()
        .flat_map(|extraction| &extraction.payload.methods)
        .map(length)
        .sum()
}

fn current_rss_kib() -> Option<u64> {
    let output = Command::new("ps")
        .args(["-o", "rss=", "-p", &std::process::id().to_string()])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8(output.stdout).ok()?.trim().parse().ok()
}

fn update_max_rss(maximum: &mut Option<u64>) {
    if let Some(current) = current_rss_kib() {
        *maximum = Some(maximum.map_or(current, |value| value.max(current)));
    }
}

fn nonce_for_index(index: usize) -> [u8; 12] {
    let mut nonce = [0x27u8; 12];
    nonce[8..].copy_from_slice(&(index as u32).to_le_bytes());
    nonce
}

impl fmt::Display for BenchmarkReport {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(formatter, "策略\t{}", self.strategy)?;
        writeln!(formatter, "DEX 数量\t{}", self.dex_count)?;
        writeln!(formatter, "原始 DEX 字节\t{}", self.original_bytes)?;
        writeln!(formatter, "占位载体字节\t{}", self.carrier_bytes)?;
        writeln!(
            formatter,
            "抽取指令字节\t{}",
            self.extracted_instruction_bytes
        )?;
        writeln!(formatter, "占位指令字节\t{}", self.stub_instruction_bytes)?;
        writeln!(formatter, "认证密文字节\t{}", self.ciphertext_bytes)?;
        writeln!(
            formatter,
            "读取耗时毫秒\t{}",
            self.read_duration.as_millis()
        )?;
        writeln!(
            formatter,
            "抽取耗时毫秒\t{}",
            self.extract_duration.as_millis()
        )?;
        writeln!(
            formatter,
            "封装耗时毫秒\t{}",
            self.seal_duration.as_millis()
        )?;
        writeln!(
            formatter,
            "解封恢复耗时毫秒\t{}",
            self.open_duration.as_millis()
        )?;
        writeln!(
            formatter,
            "读取后 RSS KiB\t{}",
            optional_metric(self.rss_after_read_kib)
        )?;
        writeln!(
            formatter,
            "抽取后 RSS KiB\t{}",
            optional_metric(self.rss_after_extract_kib)
        )?;
        writeln!(
            formatter,
            "封装后 RSS KiB\t{}",
            optional_metric(self.rss_after_seal_kib)
        )?;
        write!(
            formatter,
            "解封后 RSS KiB\t{}",
            optional_metric(self.rss_after_open_kib)
        )
    }
}

fn optional_metric(value: Option<u64>) -> String {
    value
        .map(|value| value.to_string())
        .unwrap_or_else(|| "不可用".to_owned())
}

impl fmt::Display for LayeredReport {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(formatter, "DEX 数量\t{}", self.dex_count)?;
        writeln!(
            formatter,
            "认证总索引密文字节\t{}",
            self.index_ciphertext_bytes
        )?;
        write!(
            formatter,
            "逐 DEX 分片密文字节\t{}",
            self.shard_ciphertext_bytes
        )
    }
}

#[cfg(test)]
mod tests {
    use super::dex_sequence;
    use std::path::Path;

    #[test]
    fn dex_文件名使用自然顺序() {
        assert_eq!(dex_sequence(Path::new("classes.dex")), Some(1));
        assert_eq!(dex_sequence(Path::new("classes2.dex")), Some(2));
        assert_eq!(dex_sequence(Path::new("classes12.dex")), Some(12));
        assert_eq!(dex_sequence(Path::new("other.dex")), None);
    }
}

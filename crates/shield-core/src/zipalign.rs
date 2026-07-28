use anyhow::{Context, Result};
use std::fs;
use std::io::{self, Read, Write};
use std::path::Path;
use tempfile::NamedTempFile;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, DateTime, ZipArchive, ZipWriter};

const DEFAULT_ALIGNMENT: u16 = 4;
const SHARED_LIB_ALIGNMENT: u16 = 16 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AlignmentIssue {
    pub name: String,
    pub required: u64,
    pub actual: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NativeLibraryPackaging {
    Preserve,
    Store,
}

fn is_native_library(name: &str) -> bool {
    name.starts_with("lib/") && name.ends_with(".so")
}

fn required_alignment(name: &str) -> u16 {
    if is_native_library(name) {
        SHARED_LIB_ALIGNMENT
    } else {
        DEFAULT_ALIGNMENT
    }
}

fn is_directory_name(name: &str) -> bool {
    name.ends_with('/')
}

fn build_file_options(
    file: &zip::read::ZipFile<'_>,
    alignment: u16,
    native_packaging: NativeLibraryPackaging,
) -> SimpleFileOptions {
    let compression =
        if native_packaging == NativeLibraryPackaging::Store && is_native_library(file.name()) {
            CompressionMethod::Stored
        } else {
            file.compression()
        };
    let mut options = SimpleFileOptions::default()
        .compression_method(compression)
        .last_modified_time(
            file.last_modified()
                .filter(DateTime::is_valid)
                .unwrap_or_else(DateTime::default_for_write),
        )
        .with_alignment(alignment);

    if file.compressed_size().max(file.size()) > u32::MAX as u64 {
        options = options.large_file(true);
    }

    if let Some(mode) = file.unix_mode() {
        options = options.unix_permissions(mode);
    }

    options
}

pub fn verify_apk_alignment(path: &Path) -> Result<Vec<AlignmentIssue>> {
    let file =
        fs::File::open(path).with_context(|| format!("打开 APK 失败: {}", path.display()))?;
    let mut archive = ZipArchive::new(file)
        .with_context(|| format!("解析 APK ZIP 结构失败: {}", path.display()))?;
    let mut issues = Vec::new();

    for i in 0..archive.len() {
        let file = archive
            .by_index(i)
            .with_context(|| format!("读取 ZIP 条目失败: index={i}"))?;
        if file.is_dir() || is_directory_name(file.name()) {
            continue;
        }

        let required = required_alignment(file.name()) as u64;
        let actual = file.data_start() % required;
        if actual != 0 {
            issues.push(AlignmentIssue {
                name: file.name().to_string(),
                required,
                actual,
            });
        }
    }

    Ok(issues)
}

pub fn align_apk(path: &Path) -> Result<()> {
    align_apk_with_native_packaging(path, NativeLibraryPackaging::Preserve)
}

pub(crate) fn align_apk_with_native_packaging(
    path: &Path,
    native_packaging: NativeLibraryPackaging,
) -> Result<()> {
    let parent = path
        .parent()
        .with_context(|| format!("无法确定 APK 父目录: {}", path.display()))?;
    let mut temp = NamedTempFile::new_in(parent)
        .with_context(|| format!("创建临时对齐文件失败: {}", parent.display()))?;

    rewrite_aligned(path, temp.as_file_mut(), native_packaging)?;
    temp.as_file_mut()
        .flush()
        .context("刷新对齐后的 APK 失败")?;
    if path.exists() {
        fs::remove_file(path).with_context(|| format!("删除旧 APK 失败: {}", path.display()))?;
    }

    temp.persist(path).map_err(|e| {
        anyhow::anyhow!(
            "替换对齐后的 APK 失败: {} -> {}: {}",
            e.file.path().display(),
            path.display(),
            e.error
        )
    })?;

    let issues = verify_apk_alignment(path).context("校验 APK 对齐结果失败")?;
    if !issues.is_empty() {
        anyhow::bail!(
            "APK 对齐结果校验失败，仍存在未对齐条目：{}",
            format_alignment_issues(&issues)
        );
    }

    if native_packaging == NativeLibraryPackaging::Store {
        let compressed = find_compressed_native_libraries(path)?;
        if !compressed.is_empty() {
            anyhow::bail!(
                "Native 库打包与 extractNativeLibs=false 不一致，仍存在压缩条目：{}",
                compressed
                    .into_iter()
                    .take(5)
                    .collect::<Vec<_>>()
                    .join("；")
            );
        }
    }

    Ok(())
}

pub(crate) fn find_compressed_native_libraries(path: &Path) -> Result<Vec<String>> {
    let file =
        fs::File::open(path).with_context(|| format!("打开 APK 失败: {}", path.display()))?;
    let mut archive = ZipArchive::new(file)
        .with_context(|| format!("解析 APK ZIP 结构失败: {}", path.display()))?;
    let mut compressed = Vec::new();

    for index in 0..archive.len() {
        let entry = archive
            .by_index(index)
            .with_context(|| format!("读取 ZIP 条目失败: index={index}"))?;
        if is_native_library(entry.name()) && entry.compression() != CompressionMethod::Stored {
            compressed.push(entry.name().to_string());
        }
    }

    Ok(compressed)
}

fn format_alignment_issues(issues: &[AlignmentIssue]) -> String {
    let mut parts = issues
        .iter()
        .take(5)
        .map(|issue| {
            format!(
                "{} 需要 {} 字节对齐，当前偏移余数 {}",
                issue.name, issue.required, issue.actual
            )
        })
        .collect::<Vec<_>>();

    if issues.len() > 5 {
        parts.push(format!("另有 {} 个条目未对齐", issues.len() - 5));
    }

    parts.join("；")
}

fn rewrite_aligned(
    path: &Path,
    output: &mut fs::File,
    native_packaging: NativeLibraryPackaging,
) -> Result<()> {
    let input =
        fs::File::open(path).with_context(|| format!("打开 APK 失败: {}", path.display()))?;
    let mut archive = ZipArchive::new(input)
        .with_context(|| format!("解析 APK ZIP 结构失败: {}", path.display()))?;

    let mut writer = ZipWriter::new(io::BufWriter::new(output));
    writer.set_raw_comment(archive.comment().to_vec().into_boxed_slice());
    if let Some(comment) = archive.zip64_comment() {
        writer.set_raw_zip64_comment(Some(comment.to_vec().into_boxed_slice()));
    }

    let mut buffer = vec![0u8; 64 * 1024];

    for i in 0..archive.len() {
        let mut file = archive
            .by_index(i)
            .with_context(|| format!("读取 ZIP 条目失败: index={i}"))?;
        let name = file.name().to_string();
        let alignment = required_alignment(&name);
        let options = build_file_options(&file, alignment, native_packaging);

        if file.is_dir() || is_directory_name(&name) {
            writer
                .add_directory(name, options)
                .context("写入目录条目失败")?;
            continue;
        }

        writer.start_file(name, options).context("写入文件头失败")?;
        loop {
            let read = file.read(&mut buffer).context("读取 ZIP 条目数据失败")?;
            if read == 0 {
                break;
            }
            writer
                .write_all(&buffer[..read])
                .context("写入对齐后的 ZIP 数据失败")?;
        }
    }

    writer.finish().context("完成 APK 对齐写入失败")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::process::Command;
    use tempfile::tempdir;

    fn make_sample_apk(path: &Path) {
        let file = fs::File::create(path).unwrap();
        let mut writer = ZipWriter::new(file);

        writer
            .start_file(
                "AndroidManifest.xml",
                SimpleFileOptions::default().compression_method(CompressionMethod::Deflated),
            )
            .unwrap();
        writer.write_all(b"<manifest />").unwrap();

        writer
            .start_file(
                "lib/arm64-v8a/libdemo.so",
                SimpleFileOptions::default().compression_method(CompressionMethod::Deflated),
            )
            .unwrap();
        writer.write_all(&vec![0x5Au8; 8192]).unwrap();

        writer
            .start_file(
                "res/raw/payload.bin",
                SimpleFileOptions::default().compression_method(CompressionMethod::Stored),
            )
            .unwrap();
        writer.write_all(b"payload").unwrap();

        writer.finish().unwrap();
    }

    fn find_system_zipalign() -> Option<PathBuf> {
        if let Ok(path) = std::env::var("ANDROID_ZIPALIGN") {
            let candidate = PathBuf::from(path);
            if candidate.exists() {
                return Some(candidate);
            }
        }

        let mut roots = Vec::new();
        if let Ok(root) = std::env::var("ANDROID_SDK_ROOT") {
            roots.push(PathBuf::from(root));
        }
        if let Ok(root) = std::env::var("ANDROID_HOME") {
            roots.push(PathBuf::from(root));
        }
        if let Ok(home) = std::env::var("HOME") {
            let home = PathBuf::from(home);
            roots.push(home.join("Library/Android/sdk"));
            roots.push(home.join("Android/Sdk"));
        }

        for root in roots {
            let build_tools = root.join("build-tools");
            if !build_tools.exists() {
                continue;
            }

            let Ok(entries) = fs::read_dir(&build_tools) else {
                continue;
            };

            let mut candidates = entries
                .filter_map(|entry| entry.ok().map(|it| it.path().join("zipalign")))
                .filter(|path| path.exists())
                .collect::<Vec<_>>();
            candidates.sort();
            if let Some(path) = candidates.pop() {
                return Some(path);
            }
        }

        None
    }

    #[test]
    fn required_alignment_matches_android_zipalign_rule() {
        assert_eq!(
            required_alignment("lib/arm64-v8a/libdemo.so"),
            SHARED_LIB_ALIGNMENT
        );
        assert_eq!(
            required_alignment("lib/x86_64/libdemo.so"),
            SHARED_LIB_ALIGNMENT
        );
        assert_eq!(required_alignment("classes.dex"), DEFAULT_ALIGNMENT);
        assert_eq!(required_alignment("assets/libdemo.so"), DEFAULT_ALIGNMENT);
        assert_eq!(
            required_alignment("lib/arm64-v8a/libdemo.txt"),
            DEFAULT_ALIGNMENT
        );
    }

    #[test]
    fn verify_detects_unaligned_entries() {
        let dir = tempdir().unwrap();
        let apk = dir.path().join("sample.apk");
        make_sample_apk(&apk);

        let issues = verify_apk_alignment(&apk).unwrap();
        assert!(!issues.is_empty());
        assert!(issues
            .iter()
            .any(|it| it.name == "lib/arm64-v8a/libdemo.so"));
    }

    #[test]
    fn align_apk_rewrites_entries_with_expected_alignment() {
        let dir = tempdir().unwrap();
        let apk = dir.path().join("sample.apk");
        make_sample_apk(&apk);

        align_apk(&apk).unwrap();

        let issues = verify_apk_alignment(&apk).unwrap();
        assert!(issues.is_empty(), "仍存在未对齐条目: {issues:?}");

        let file = fs::File::open(&apk).unwrap();
        let mut archive = ZipArchive::new(file).unwrap();

        let so_offset = archive
            .by_name("lib/arm64-v8a/libdemo.so")
            .unwrap()
            .data_start();
        assert_eq!(so_offset % SHARED_LIB_ALIGNMENT as u64, 0);

        let payload_offset = archive.by_name("res/raw/payload.bin").unwrap().data_start();
        assert_eq!(payload_offset % DEFAULT_ALIGNMENT as u64, 0);
    }

    #[test]
    fn store_native_libraries_rewrites_only_so_as_stored() {
        let dir = tempdir().unwrap();
        let apk = dir.path().join("sample.apk");
        make_sample_apk(&apk);

        align_apk_with_native_packaging(&apk, NativeLibraryPackaging::Store).unwrap();

        assert!(find_compressed_native_libraries(&apk).unwrap().is_empty());
        let file = fs::File::open(&apk).unwrap();
        let mut archive = ZipArchive::new(file).unwrap();
        assert_eq!(
            archive
                .by_name("lib/arm64-v8a/libdemo.so")
                .unwrap()
                .compression(),
            CompressionMethod::Stored
        );
        assert_eq!(
            archive
                .by_name("AndroidManifest.xml")
                .unwrap()
                .compression(),
            CompressionMethod::Deflated
        );
    }

    #[test]
    fn preserve_native_libraries_keeps_compression_method() {
        let dir = tempdir().unwrap();
        let apk = dir.path().join("sample.apk");
        make_sample_apk(&apk);

        align_apk(&apk).unwrap();

        assert_eq!(
            find_compressed_native_libraries(&apk).unwrap(),
            vec!["lib/arm64-v8a/libdemo.so"]
        );
    }

    #[test]
    fn align_apk_preserves_entry_contents() {
        let dir = tempdir().unwrap();
        let apk = dir.path().join("sample.apk");
        make_sample_apk(&apk);

        align_apk(&apk).unwrap();

        let file = fs::File::open(&apk).unwrap();
        let mut archive = ZipArchive::new(file).unwrap();

        let mut manifest = String::new();
        archive
            .by_name("AndroidManifest.xml")
            .unwrap()
            .read_to_string(&mut manifest)
            .unwrap();
        assert_eq!(manifest, "<manifest />");

        let mut so = Vec::new();
        archive
            .by_name("lib/arm64-v8a/libdemo.so")
            .unwrap()
            .read_to_end(&mut so)
            .unwrap();
        assert_eq!(so.len(), 8192);
        assert!(so.iter().all(|b| *b == 0x5A));
    }

    #[test]
    fn format_alignment_issues_limits_output() {
        let issues = (0..7)
            .map(|index| AlignmentIssue {
                name: format!("lib/arm64-v8a/libdemo{index}.so"),
                required: SHARED_LIB_ALIGNMENT as u64,
                actual: index + 1,
            })
            .collect::<Vec<_>>();

        let message = format_alignment_issues(&issues);
        assert!(message.contains("libdemo0.so"));
        assert!(message.contains("另有 2 个条目未对齐"));
        assert!(!message.contains("libdemo6.so 需要"));
    }

    #[test]
    fn align_apk_matches_official_zipalign_verification() {
        let Some(zipalign) = find_system_zipalign() else {
            eprintln!("跳过交叉验证：未找到系统 zipalign");
            return;
        };

        let dir = tempdir().unwrap();
        let apk = dir.path().join("sample.apk");
        make_sample_apk(&apk);

        align_apk(&apk).unwrap();

        let output = Command::new(&zipalign)
            .args(["-c", "-P", "16", "4"])
            .arg(&apk)
            .output()
            .unwrap();

        assert!(
            output.status.success(),
            "官方 zipalign 校验失败: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

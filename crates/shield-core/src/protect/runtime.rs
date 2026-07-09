use anyhow::{Context, Result};
use std::collections::HashSet;
use std::fs;
use std::path::Path;

use crate::protect::dex::patch_dex_header;
use crate::utils::{human_size, print_success};

pub(crate) fn inject_runtime(
    apk_dir: &Path,
    runtime_resources: &Path,
    original_apk: &Path,
) -> Result<()> {
    let supported_abis = collect_apk_abis(original_apk);

    let file = fs::File::open(runtime_resources)?;
    let mut archive = zip::ZipArchive::new(file)?;

    for i in 0..archive.len() {
        let mut file = archive.by_index(i)?;
        let file_name = file.name().to_string();

        if file_name.contains("libzstd-jni") {
            continue;
        }

        if !supported_abis.is_empty() && file_name.starts_with("lib/") {
            let abi = file_name.split('/').nth(1).unwrap_or("");
            if !abi.is_empty() && !supported_abis.contains(abi) {
                continue;
            }
        }

        let outpath = apk_dir.join(&file_name);
        if file_name.ends_with('/') {
            fs::create_dir_all(&outpath)?;
        } else {
            if let Some(p) = outpath.parent() {
                fs::create_dir_all(p)?;
            }
            let mut outfile = fs::File::create(&outpath)?;
            std::io::copy(&mut file, &mut outfile)?;
        }
    }

    let stub_dex = apk_dir.join("stub-classes.dex");
    let classes_dex = apk_dir.join("classes.dex");

    if stub_dex.exists() {
        fs::rename(&stub_dex, &classes_dex).context("重命名stub-classes.dex失败")?;
        print_success("stub-classes.dex -> classes.dex");
    } else {
        anyhow::bail!("未找到stub-classes.dex");
    }

    let tmp_bin = apk_dir.parent().unwrap().join("app.bin.tmp");
    if !tmp_bin.exists() {
        anyhow::bail!("未找到临时 app.bin.tmp，process_dex 可能未执行");
    }
    let payload = fs::read(&tmp_bin).context("读取 app.bin.tmp 失败")?;
    if let Err(e) = fs::remove_file(&tmp_bin) {
        log::warn!("删除临时文件 app.bin.tmp 失败（不影响加固结果）: {}", e);
    }

    let payload_len =
        u32::try_from(payload.len()).context("payload 超过 4GiB 上限，无法写入 u32 长度字段")?;

    let original_dex_size = {
        let header = fs::read(&classes_dex).context("读取 classes.dex header 失败")?;
        if header.len() < 12 {
            anyhow::bail!("classes.dex 过短，无法读取 DEX file_size 字段");
        }
        u32::from_le_bytes(header[8..12].try_into().unwrap()) as u64
    };
    let current_size = fs::metadata(&classes_dex)?.len();
    if current_size > original_dex_size {
        let file = fs::OpenOptions::new()
            .write(true)
            .open(&classes_dex)
            .context("打开 classes.dex 裁剪失败")?;
        file.set_len(original_dex_size)
            .context("裁剪 classes.dex 到原始 file_size 失败")?;
        log::warn!(
            "classes.dex 已有尾部追加数据（当前 {} 字节，原始 {} 字节），已裁剪",
            current_size,
            original_dex_size
        );
    }

    let mut classes_dex_file = fs::OpenOptions::new()
        .append(true)
        .open(&classes_dex)
        .context("打开 classes.dex 追加写入失败")?;
    use std::io::Write;
    classes_dex_file
        .write_all(b"MSHD")
        .context("写入 MSHD magic 失败")?;
    classes_dex_file
        .write_all(&payload_len.to_le_bytes())
        .context("写入 payload 长度失败")?;
    classes_dex_file
        .write_all(&payload)
        .context("写入 payload 失败")?;

    drop(classes_dex_file);
    patch_dex_header(&classes_dex).context("修复 classes.dex DEX header 失败")?;

    let classes_dex_size = fs::metadata(&classes_dex)?.len();
    print_success(&format!(
        "加密 DEX 已追加到 classes.dex 末尾（总大小 {}）",
        human_size(classes_dex_size)
    ));
    print_success("Runtime资源注入完成");
    Ok(())
}

pub(crate) fn read_stub_application(resources_path: &Path) -> Result<String> {
    use std::io::Read;

    let file = fs::File::open(resources_path).context("打开 resources.zip 失败")?;
    let mut archive = zip::ZipArchive::new(file).context("解析 resources.zip 失败")?;
    let mut entry = archive
        .by_name("metadata.json")
        .context("resources.zip 中未找到 metadata.json")?;
    let mut content = String::new();
    entry
        .read_to_string(&mut content)
        .context("读取 metadata.json 失败")?;

    parse_json_string_field(&content, "stub_application")
        .context("metadata.json 中未找到 stub_application 字段")
}

fn parse_json_string_field(json: &str, field: &str) -> Option<String> {
    let needle = format!("\"{}\"", field);
    let start = json.find(&needle)?;
    let after_key = &json[start + needle.len()..];
    let after_colon = after_key.trim_start().strip_prefix(':')?.trim_start();
    let after_quote = after_colon.strip_prefix('"')?;
    let end = after_quote.find('"')?;
    Some(after_quote[..end].to_string())
}

fn collect_apk_abis(apk_path: &Path) -> HashSet<String> {
    let mut abis = HashSet::new();
    let Ok(file) = fs::File::open(apk_path) else {
        return abis;
    };
    let Ok(mut archive) = zip::ZipArchive::new(file) else {
        return abis;
    };
    for i in 0..archive.len() {
        let Ok(entry) = archive.by_index(i) else {
            continue;
        };
        let name = entry.name();
        if name.starts_with("lib/") && name.len() > 4 {
            if let Some(abi) = name.split('/').nth(1) {
                if !abi.is_empty() {
                    abis.insert(abi.to_string());
                }
            }
        }
    }
    abis
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write as _;

    #[test]
    fn parse_json_string_field_found() {
        let json = r#"{"stub_application": "msk.b", "version": "5"}"#;
        assert_eq!(
            parse_json_string_field(json, "stub_application"),
            Some("msk.b".to_string())
        );
    }

    #[test]
    fn parse_json_string_field_not_found_returns_none() {
        let json = r#"{"version": "5"}"#;
        assert_eq!(parse_json_string_field(json, "stub_application"), None);
    }

    #[test]
    fn collect_apk_abis_detects_multiple_abis() {
        let dir = tempfile::tempdir().unwrap();
        let apk_path = dir.path().join("test.apk");
        {
            let f = fs::File::create(&apk_path).unwrap();
            let mut zip = zip::ZipWriter::new(f);
            let opts = zip::write::SimpleFileOptions::default();
            zip.start_file("lib/arm64-v8a/libtest.so", opts).unwrap();
            zip.write_all(b"elf").unwrap();
            zip.start_file("lib/armeabi-v7a/libtest.so", opts).unwrap();
            zip.write_all(b"elf").unwrap();
            zip.start_file("classes.dex", opts).unwrap();
            zip.write_all(b"dex").unwrap();
            zip.finish().unwrap();
        }
        let abis = collect_apk_abis(&apk_path);
        assert!(abis.contains("arm64-v8a"));
        assert!(abis.contains("armeabi-v7a"));
        assert!(!abis.contains("x86"));
    }

    #[test]
    fn collect_apk_abis_no_lib_dir_returns_empty() {
        let dir = tempfile::tempdir().unwrap();
        let apk_path = dir.path().join("test.apk");
        {
            let f = fs::File::create(&apk_path).unwrap();
            let mut zip = zip::ZipWriter::new(f);
            let opts = zip::write::SimpleFileOptions::default();
            zip.start_file("classes.dex", opts).unwrap();
            zip.write_all(b"dex").unwrap();
            zip.finish().unwrap();
        }
        let abis = collect_apk_abis(&apk_path);
        assert!(abis.is_empty());
    }

    #[test]
    fn collect_apk_abis_nonexistent_path_returns_empty() {
        let abis = collect_apk_abis(std::path::Path::new("/nonexistent/path.apk"));
        assert!(abis.is_empty());
    }
}

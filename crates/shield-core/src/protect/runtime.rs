use anyhow::{Context, Result};
use std::collections::HashSet;
use std::fs;
use std::io::Read;
use std::path::Path;

use crate::protect::dex::patch_dex_header;
use crate::protect::native_alias::{
    inspect_original_apk, map_resource_path, patch_stub_dex, InjectedNativeRuntime, NativeAlias,
    NativeAliasProtocol,
};
use crate::utils::{human_size, print_success};

pub(crate) fn inject_runtime(
    apk_dir: &Path,
    runtime_resources: &Path,
    original_apk: &Path,
) -> Result<InjectedNativeRuntime> {
    let original_native = inspect_original_apk(original_apk)?;

    let file = fs::File::open(runtime_resources)?;
    let mut archive = zip::ZipArchive::new(file)?;
    let metadata = read_archive_text(&mut archive, "metadata.json")?;
    let protocol = NativeAliasProtocol::parse(&metadata)?;
    let alias = NativeAlias::generate(&original_native)?;
    let alias_file_name = alias.file_name();
    let mut injected_abis = HashSet::new();

    for i in 0..archive.len() {
        let mut file = archive.by_index(i)?;
        let file_name = file.name().to_string();

        if file_name == "metadata.json" || file_name.contains("libzstd-jni") {
            continue;
        }

        if !original_native.abis.is_empty() && file_name.starts_with("lib/") {
            let abi = file_name.split('/').nth(1).unwrap_or("");
            if !abi.is_empty() && !original_native.abis.contains(abi) {
                continue;
            }
        }

        let output_name =
            map_resource_path(&file_name, &protocol.canonical_file_name, &alias_file_name);
        if output_name != file_name {
            if let Some(abi) = file_name.split('/').nth(1) {
                if !injected_abis.insert(abi.to_string()) {
                    anyhow::bail!("Runtime 资源存在重复的 Native 规范库: {file_name}");
                }
            }
        }

        let outpath = apk_dir.join(&output_name);
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
    if injected_abis.is_empty() {
        anyhow::bail!("Runtime 资源中未找到可注入的 Native 规范库");
    }
    let expected_abis = if original_native.abis.is_empty() {
        HashSet::from([
            "arm64-v8a".to_string(),
            "armeabi-v7a".to_string(),
            "x86".to_string(),
            "x86_64".to_string(),
        ])
    } else {
        original_native.abis
    };
    if injected_abis != expected_abis {
        anyhow::bail!(
            "Runtime Native ABI 不完整: 期望 {:?}，实际 {:?}",
            expected_abis,
            injected_abis
        );
    }
    patch_stub_dex(&classes_dex, &protocol, &alias)?;

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
    print_success("Runtime Native 别名已生成");
    print_success("Runtime资源注入完成");
    Ok(InjectedNativeRuntime {
        alias_file_name,
        injected_abis,
        placeholder: protocol.placeholder,
        canonical_file_name: protocol.canonical_file_name,
    })
}

pub(crate) fn read_stub_application(resources_path: &Path) -> Result<String> {
    let file = fs::File::open(resources_path).context("打开 resources.zip 失败")?;
    let mut archive = zip::ZipArchive::new(file).context("解析 resources.zip 失败")?;
    let content = read_archive_text(&mut archive, "metadata.json")?;

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

fn read_archive_text<R: std::io::Read + std::io::Seek>(
    archive: &mut zip::ZipArchive<R>,
    name: &str,
) -> Result<String> {
    let mut entry = archive
        .by_name(name)
        .with_context(|| format!("resources.zip 中未找到 {name}"))?;
    let mut content = String::new();
    entry
        .read_to_string(&mut content)
        .with_context(|| format!("读取 {name} 失败"))?;
    Ok(content)
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
    fn inspect_original_apk_detects_abis_and_names() {
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
        let native = inspect_original_apk(&apk_path).unwrap();
        assert!(native.abis.contains("arm64-v8a"));
        assert!(native.abis.contains("armeabi-v7a"));
        assert!(!native.abis.contains("x86"));
    }

    #[test]
    fn inspect_original_apk_no_lib_dir_returns_empty() {
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
        let native = inspect_original_apk(&apk_path).unwrap();
        assert!(native.abis.is_empty());
    }

    #[test]
    fn inspect_original_apk_nonexistent_path_returns_error() {
        assert!(inspect_original_apk(std::path::Path::new("/nonexistent/path.apk")).is_err());
    }

    #[test]
    fn map_resource_path_only_rewrites_canonical_library() {
        assert_eq!(
            map_resource_path(
                "lib/arm64-v8a/libmocikashield.so",
                "libmocikashield.so",
                "libnativecorebridge.so"
            ),
            "lib/arm64-v8a/libnativecorebridge.so"
        );
        assert_eq!(
            map_resource_path(
                "lib/arm64-v8a/libbusiness.so",
                "libmocikashield.so",
                "libnativecorebridge.so"
            ),
            "lib/arm64-v8a/libbusiness.so"
        );
    }

    #[test]
    fn inject_runtime_rewrites_native_paths_and_stub_placeholder() {
        let dir = tempfile::tempdir().unwrap();
        let apk_dir = dir.path().join("apk");
        fs::create_dir_all(&apk_dir).unwrap();
        fs::write(dir.path().join("app.bin.tmp"), b"payload").unwrap();

        let original_apk = dir.path().join("original.apk");
        {
            let file = fs::File::create(&original_apk).unwrap();
            let mut zip = zip::ZipWriter::new(file);
            zip.start_file("classes.dex", zip::write::SimpleFileOptions::default())
                .unwrap();
            zip.write_all(b"dex").unwrap();
            zip.finish().unwrap();
        }

        let resources = dir.path().join("resources.zip");
        {
            let file = fs::File::create(&resources).unwrap();
            let mut zip = zip::ZipWriter::new(file);
            let options = zip::write::SimpleFileOptions::default();
            let metadata = r#"{
                "native_library":"libmocikashield.so",
                "native_name_placeholder":"mocikanativeslot",
                "native_name_length":16,
                "native_name_scheme":1
            }"#;
            zip.start_file("metadata.json", options).unwrap();
            zip.write_all(metadata.as_bytes()).unwrap();
            let mut dex = vec![0u8; 96];
            dex[0..8].copy_from_slice(b"dex\n035\0");
            dex[32..36].copy_from_slice(&(96u32).to_le_bytes());
            dex[48..64].copy_from_slice(b"mocikanativeslot");
            zip.start_file("stub-classes.dex", options).unwrap();
            zip.write_all(&dex).unwrap();
            for abi in ["arm64-v8a", "armeabi-v7a", "x86", "x86_64"] {
                zip.start_file(format!("lib/{abi}/libmocikashield.so"), options)
                    .unwrap();
                zip.write_all(abi.as_bytes()).unwrap();
            }
            zip.finish().unwrap();
        }

        let injected = inject_runtime(&apk_dir, &resources, &original_apk).unwrap();
        assert_eq!(injected.injected_abis.len(), 4);
        assert!(!apk_dir.join("metadata.json").exists());
        for abi in ["arm64-v8a", "armeabi-v7a", "x86", "x86_64"] {
            assert!(apk_dir
                .join("lib")
                .join(abi)
                .join(&injected.alias_file_name)
                .exists());
            assert!(!apk_dir
                .join("lib")
                .join(abi)
                .join("libmocikashield.so")
                .exists());
        }
        let dex = fs::read(apk_dir.join("classes.dex")).unwrap();
        assert!(!dex.windows(16).any(|value| value == b"mocikanativeslot"));
    }
}

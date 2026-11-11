//! 真实二进制清单 APK、内容快照及原工具独立验签；仅用于显式专项。
use super::*;
use std::{collections::BTreeMap, path::PathBuf};

pub(super) fn input(root: &Path, name: &str) -> PathBuf {
    let source =
        std::env::var_os("SHIELD_TEST_APK").expect("指定自有真实 APK，不能使用合成 ZIP 替代");
    let path = root.join(name);
    fs::copy(Path::new(&source), &path).unwrap();
    let mut zip = zip::ZipArchive::new(fs::File::open(&path).unwrap()).unwrap();
    let mut manifest = Vec::new();
    zip.by_name("AndroidManifest.xml")
        .unwrap()
        .read_to_end(&mut manifest)
        .unwrap();
    assert_eq!(
        manifest.get(..4),
        Some(&[3, 0, 8, 0][..]),
        "需要真实二进制清单 APK"
    );
    drop(zip);
    // 仅验证 ZIP 存储／对齐，不把这些 Native 标记字节宣称为有效 ELF 或设备运行证据。
    let mut zip = zip::ZipWriter::new_append(
        fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&path)
            .unwrap(),
    )
    .unwrap();
    for (name, method) in [
        (
            "assets/adapter-signing-check/中文内容.bin",
            zip::CompressionMethod::Deflated,
        ),
        (
            "lib/arm64-v8a/libadaptercheck.so",
            zip::CompressionMethod::Stored,
        ),
        (
            "assets/adapter-signing-check/stored.bin",
            zip::CompressionMethod::Stored,
        ),
    ] {
        zip.start_file(
            name,
            zip::write::SimpleFileOptions::default().compression_method(method),
        )
        .unwrap();
        zip.write_all(&vec![0x53; 2049]).unwrap();
    }
    zip.finish().unwrap();
    path
}

pub(super) fn contents(path: &Path) -> BTreeMap<String, ([u8; 32], zip::CompressionMethod)> {
    let mut zip = zip::ZipArchive::new(fs::File::open(path).unwrap()).unwrap();
    let mut result = BTreeMap::new();
    for index in 0..zip.len() {
        let mut entry = zip.by_index(index).unwrap();
        if entry.name().starts_with("META-INF/") || entry.is_dir() {
            continue;
        }
        let mut bytes = Vec::new();
        entry.read_to_end(&mut bytes).unwrap();
        assert!(
            result
                .insert(
                    entry.name().to_owned(),
                    (Sha256::digest(bytes).into(), entry.compression())
                )
                .is_none(),
            "真实样本不应包含重复业务条目"
        );
    }
    result
}

pub(super) fn independently_verify(root: &Path, java: &Path, source: &Path, fingerprint: &str) {
    let verify = root.join("independent-verify.apk");
    fs::copy(source, &verify).unwrap();
    let signer = crate::utils::find_apksigner().unwrap();
    let result = java_command(java)
        .arg("-jar")
        .arg(&signer)
        .args(["verify", "--verbose", "--print-certs"])
        .arg(&verify)
        .output()
        .unwrap();
    assert!(result.status.success(), "独立原工具未验证真实 APK 签名");
    let text = std::str::from_utf8(&result.stdout).unwrap();
    assert!(text.to_ascii_uppercase().contains(fingerprint));
    // 默认验证按 APK 的 minSdkVersion 决定需要检查的方案；高最低版本会跳过 V1。
    // 额外扩大密码学验证区间，不修改 APK，也不据此宣称应用能运行于 API 21。
    let all_schemes = java_command(java)
        .arg("-jar")
        .arg(&signer)
        .args([
            "verify",
            "--min-sdk-version",
            "21",
            "--verbose",
            "--print-certs",
        ])
        .arg(&verify)
        .output()
        .unwrap();
    assert!(
        all_schemes.status.success(),
        "扩大区间后 V1～V3 独立验签失败"
    );
    let text = std::str::from_utf8(&all_schemes.stdout).unwrap();
    assert!(text.to_ascii_uppercase().contains(fingerprint));
    for scheme in ["v1", "v2", "v3"] {
        assert!(
            text.lines().any(
                |line| line.starts_with(&format!("Verified using {scheme} scheme"))
                    && line.ends_with(": true")
            ),
            "独立验签未证明签名方案有效"
        );
    }
    let mut archive = zip::ZipArchive::new(fs::File::open(source).unwrap()).unwrap();
    for issue in crate::verify_apk_alignment(source).unwrap() {
        // 与现有执行器一致：安装偏移约束作用于 Stored；签名器新增的压缩条目不要求偏移对齐。
        assert_ne!(
            archive.by_name(&issue.name).unwrap().compression(),
            zip::CompressionMethod::Stored,
            "未压缩条目必须保持对齐：{}",
            issue.name
        );
    }
    let zipalign =
        std::env::var_os("SHIELD_TEST_ZIPALIGN").expect("指定独立 Android SDK 对齐验证工具");
    assert!(
        crate::utils::no_window_command(zipalign)
            .args(["-c", "-P", "16", "4"])
            .arg(&verify)
            .output()
            .unwrap()
            .status
            .success(),
        "原 SDK 对齐验证失败"
    );
}

pub(super) fn requests_without_secrets(root: &Path, secrets: &[&str]) {
    let mut found = false;
    for entry in walkdir::WalkDir::new(root) {
        let entry = entry.unwrap();
        if entry.file_name() != "request.bin" {
            continue;
        }
        found = true;
        let bytes = fs::read(entry.path()).unwrap();
        for secret in secrets {
            assert!(!secret.is_empty());
            assert!(
                !bytes
                    .windows(secret.len())
                    .any(|part| part == secret.as_bytes()),
                "口令不得落入请求文件"
            );
        }
    }
    assert!(found, "必须实际经过正式请求适配");
}

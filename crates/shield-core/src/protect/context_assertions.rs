//! 从最终 APK 独立检查 DEX 恢复、证书绑定和 Native 打包，不作为设备运行证据。
use super::{context_fixture::*, *};
use chacha20poly1305::{
    aead::{Aead, KeyInit},
    ChaCha20Poly1305,
};
use sha2::{Digest, Sha256};
use std::{io::Cursor, path::Path};

fn decrypt(payload: &[u8], fingerprint: &str) -> Result<Vec<u8>, chacha20poly1305::Error> {
    let signature_end = 13 + payload[12] as usize;
    let ikm_end = signature_end + 1 + payload[signature_end] as usize;
    let nonce = &payload[ikm_end..ikm_end + 12];
    let hkdf = hkdf::Hkdf::<Sha256>::new(Some(nonce), &payload[signature_end + 1..ikm_end]);
    let mut key = [0; 32];
    hkdf.expand(fingerprint.as_bytes(), &mut key).unwrap();
    ChaCha20Poly1305::new((&key).into()).decrypt(nonce.into(), &payload[ikm_end + 12..])
}

pub(super) fn verify(fixture: &Fixture, opts: &ProtectOptions, extract: bool, api19: bool) {
    let resources = opts.resources_path.as_ref().unwrap();
    let raw = entry(&opts.output, "classes.dex");
    let scan = crate::mshd::scan(&mut Cursor::new(&raw), raw.len() as u64, false, &mut || {
        Ok(())
    })
    .unwrap();
    assert!(!scan.ambiguous && !scan.damaged);
    let block = scan.block.unwrap();
    let payload = &raw[block.offset as usize + 8..];
    assert_eq!(payload.len(), block.bytes as usize);
    assert_eq!(&payload[..8], b"DEXB\x05\0\0\0");
    assert_eq!(u32::from_le_bytes(payload[8..12].try_into().unwrap()), 1);
    assert_eq!(
        &payload[13..13 + payload[12] as usize],
        fixture.fingerprint.as_bytes()
    );
    let plain = decrypt(payload, &fixture.fingerprint).unwrap();
    assert!(
        decrypt(payload, &"0".repeat(64)).is_err(),
        "错误证书不得解密原 DEX"
    );
    let end = 1 + plain[0] as usize;
    assert_eq!(&plain[1..end], b"classes.dex");
    let compressed = u32::from_le_bytes(plain[end..end + 4].try_into().unwrap()) as usize;
    let original = u32::from_le_bytes(plain[end + 4..end + 8].try_into().unwrap()) as usize;
    assert_eq!(plain.len(), end + 8 + compressed);
    let restored = zstd::decode_all(&plain[end + 8..]).unwrap();
    assert_eq!(restored.len(), original);
    assert_eq!(restored, entry(&opts.input, "classes.dex"));

    // 业务路径可以包含补充字符；引导根仍沿用正式适配明确支持的 ASCII 范围。
    let name: String = opts
        .output
        .file_stem()
        .unwrap()
        .to_str()
        .unwrap()
        .chars()
        .filter(char::is_ascii)
        .collect();
    let context = Fixture::context(&fixture.workspace(&format!("verify-{name}")));
    let project = context
        .decode_apk(&fixture.java, &fixture.apktool, &opts.output)
        .unwrap();
    let manifest = fs::read_to_string(project.directory().join("AndroidManifest.xml")).unwrap();
    let metadata = fs::read_to_string(project.directory().join("apktool.yml")).unwrap();
    assert!(metadata.contains(if api19 {
        "minSdkVersion: 19"
    } else {
        "minSdkVersion: 21"
    }));
    assert!(manifest.contains("android:name=\"msk.t\""));
    assert!(manifest.contains(&format!("android:extractNativeLibs=\"{extract}\"")));
    for (name, value) in [
        (
            "rt.app",
            "dev.mocika.shield.fixture.MainApplication".to_owned(),
        ),
        ("rt.policy", opts.environment_policy.as_str().to_owned()),
        ("rt.cache.schema", "1".to_owned()),
        ("rt.cache.count", "1".to_owned()),
    ] {
        assert!(
            manifest.contains(&format!(
                "android:name=\"{name}\" android:value=\"{value}\""
            )),
            "元数据未保留：{name}"
        );
    }
    let original_dex = fixture.root.path().join("classes.dex");
    fs::write(&original_dex, &restored).unwrap();
    let identity = crate::protect::cache_identity::calculate(&[original_dex]).unwrap();
    assert!(manifest.contains(&identity.root_sha256));
    assert_eq!(
        entry(&opts.output, "assets/中文内容😀.bin"),
        entry(&opts.input, "assets/中文内容😀.bin")
    );

    let mut zip = zip::ZipArchive::new(fs::File::open(&opts.output).unwrap()).unwrap();
    let names = zip.file_names().map(str::to_owned).collect::<Vec<_>>();
    assert!(!names.iter().any(|name| name.starts_with("lib/armeabi/")));
    let mut alias = None;
    for abi in ["armeabi-v7a", "arm64-v8a", "x86", "x86_64"] {
        let prefix = format!("lib/{abi}/");
        let native = names
            .iter()
            .filter(|name| name.starts_with(&prefix) && name.ends_with(".so"))
            .collect::<Vec<_>>();
        assert_eq!(native.len(), 2);
        let injected = native
            .iter()
            .find(|name| !name.ends_with("/liboriginalfixture.so"))
            .unwrap();
        assert!(!injected.ends_with("/libmocikashield.so"));
        let filename = injected.rsplit('/').next().unwrap();
        if let Some(previous) = alias.replace(filename.to_owned()) {
            assert_eq!(previous, filename);
        }
        assert_eq!(
            entry(&opts.output, injected),
            entry(resources, &format!("lib/{abi}/libmocikashield.so"))
        );
        let original_name = format!("{prefix}liboriginalfixture.so");
        assert_eq!(
            entry(&opts.output, &original_name),
            entry(&opts.input, &original_name)
        );
        for name in native {
            let file = zip.by_name(name).unwrap();
            if !extract {
                assert_eq!(file.compression(), zip::CompressionMethod::Stored);
                assert_eq!(file.data_start() % 16384, 0);
            } else {
                assert_eq!(file.compression(), zip::CompressionMethod::Deflated);
            }
        }
    }
    let signed = fixture
        .root
        .path()
        .join(format!("signed-{extract}-{api19}.apk"));
    fixture.sign(&context, &opts.output, &signed);
    independent(fixture, &signed);
    let mut repeated = opts.clone();
    repeated.input = signed;
    repeated.output = fixture.root.path().join("repeated-must-not-exist.apk");
    assert!(protect_apk_with_context(
        &context,
        &fixture.java,
        &repeated,
        |event| assert_eq!(event.step, ProgressStep::CheckTools),
        Arc::new(AtomicBool::new(false))
    )
    .is_err());
    assert!(!repeated.output.exists());
}

fn independent(fixture: &Fixture, source: &Path) {
    let copy = fixture.root.path().join("independent.apk");
    fs::copy(source, &copy).unwrap();
    for override_range in [false, true] {
        let mut command = crate::utils::java_command(&fixture.java);
        command
            .arg("-jar")
            .arg(&fixture.signer)
            .args(["verify", "--verbose", "--print-certs"]);
        if override_range {
            command.args(["--min-sdk-version", "21"]);
        }
        let output = command.arg(&copy).output().unwrap();
        assert!(output.status.success(), "独立原工具验签失败");
        let text = String::from_utf8(output.stdout).unwrap();
        assert!(text.to_ascii_uppercase().contains(&fixture.fingerprint));
        if override_range {
            for scheme in ["v1", "v2", "v3"] {
                assert!(text.lines().any(|line| line
                    .starts_with(&format!("Verified using {scheme} scheme"))
                    && line.ends_with(": true")));
            }
        }
    }
    let sdk = std::env::var_os("SHIELD_TEST_ZIPALIGN").expect("显式指定独立 SDK zipalign");
    assert!(crate::utils::no_window_command(sdk)
        .args(["-c", "-P", "16", "4"])
        .arg(&copy)
        .output()
        .unwrap()
        .status
        .success());
    println!("完整加固静态核验通过：证书绑定、原 DEX 恢复、缓存身份、双策略、四 ABI、压缩／16 KB 对齐及独立 V1～V3 验签；未据此宣称设备运行通过");
}

pub(super) fn digest(path: &Path) -> Vec<u8> {
    Sha256::digest(fs::read(path).unwrap()).to_vec()
}

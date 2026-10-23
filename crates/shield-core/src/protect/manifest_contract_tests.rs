use super::*;
use crate::protect::manifest_keys::{CURRENT, LEGACY};

#[test]
fn 新旧协议分别只写配套字段并保持相同缓存身份() {
    let identity = CacheIdentity {
        schema: 1,
        dex_count: 2,
        total_dex_bytes: 12345,
        root_sha256: "a".repeat(64),
    };
    for (keys, other) in [(&CURRENT, &LEGACY), (&LEGACY, &CURRENT)] {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("AndroidManifest.xml");
        fs::write(&path, r#"<manifest><application android:name="business.App" android:appComponentFactory="business.Factory" android:extractNativeLibs="false"></application></manifest>"#).unwrap();
        modify_manifest(
            directory.path(),
            "msk.a",
            Some("msk.b"),
            EnvironmentPolicy::Strict,
            keys,
        )
        .unwrap();
        add_cache_identity(directory.path(), &identity, keys).unwrap();
        add_memory_payload_metrics(directory.path(), &identity, keys).unwrap();
        let content = fs::read_to_string(path).unwrap();
        for key in [
            keys.application,
            keys.policy,
            keys.factory,
            keys.cache_schema,
            keys.cache_count,
            keys.cache_root,
            keys.payload_bytes,
        ] {
            assert_eq!(
                content.matches(&format!("android:name=\"{key}\"")).count(),
                1
            );
        }
        assert!(!content.contains(other.application));
        assert!(!content.contains(other.policy));
        assert!(content.contains("business.App"));
        assert!(content.contains("business.Factory"));
        assert!(content.contains(&identity.root_sha256));
        assert!(content.contains("bytes:12345"));
        assert!(content.contains("android:extractNativeLibs=\"false\""));
    }
}

#[test]
fn 所有保留字段占用均在写入前拒绝() {
    for keys in [&CURRENT, &LEGACY] {
        for key in [
            keys.application,
            keys.policy,
            keys.factory,
            keys.cache_schema,
            keys.cache_count,
            keys.cache_root,
            keys.payload_bytes,
        ] {
            let directory = tempfile::tempdir().unwrap();
            let path = directory.path().join("AndroidManifest.xml");
            let original = format!("<manifest><application><meta-data android:name = '{key}' android:value='business'/></application></manifest>");
            fs::write(&path, &original).unwrap();
            assert!(modify_manifest(
                directory.path(),
                "msk.a",
                None,
                EnvironmentPolicy::Compatible,
                keys
            )
            .unwrap_err()
            .to_string()
            .contains("已占用"));
            assert_eq!(fs::read_to_string(&path).unwrap(), original);
        }
    }
}

#[test]
fn 新协议不改写业务旧标识且默认应用显式写入() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("AndroidManifest.xml");
    let business =
        r#"<meta-data android:name="ORIGINAL_APPLICATION" android:value="business.Original"/>"#;
    fs::write(
        &path,
        format!("<manifest><application>{business}</application></manifest>"),
    )
    .unwrap();
    modify_manifest(
        directory.path(),
        "msk.a",
        None,
        EnvironmentPolicy::Compatible,
        &CURRENT,
    )
    .unwrap();
    let content = fs::read_to_string(path).unwrap();
    assert!(content.contains(business));
    assert!(content.contains("android.app.Application"));
    assert!(content.contains("android:name=\"rt.app\""));
}

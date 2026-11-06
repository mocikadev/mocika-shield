//! 只读资源契约：构造条目只验证协议、清单与校验和，不证明 ELF/DEX 可运行。
use shield_core::{validate_runtime_resources, EnvironmentPolicy, RuntimeProfile};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

const META: &str = r#"{
"runtime_protocol":4,"cache_schema":1,"environment_policy":true,"memory_dex":false,
"min_android_api":21,"stub_application":"msk.t","native_library":"libmocikashield.so",
"native_name_placeholder":"mocikanativeslot","native_name_length":16,"native_name_scheme":1
}"#;
const ABIS: &[&str] = &["armeabi-v7a", "arm64-v8a", "x86", "x86_64"];

fn archive(root: &Path, metadata: &str, omit: &[&str], extra: &[(&str, &[u8])]) -> PathBuf {
    let path = root.join("资源 中文.zip");
    let mut zip = zip::ZipWriter::new(fs::File::create(&path).unwrap());
    let mut files = vec![
        ("metadata.json".to_owned(), metadata.as_bytes()),
        ("stub-classes.dex".to_owned(), b"stub".as_slice()),
    ];
    files.extend(ABIS.iter().map(|abi| {
        (
            format!("lib/{abi}/libmocikashield.so"),
            b"native".as_slice(),
        )
    }));
    files.retain(|(name, _)| !omit.contains(&name.as_str()));
    files.extend(extra.iter().map(|(name, bytes)| (name.to_string(), *bytes)));
    for (name, data) in files {
        zip.start_file(
            name,
            zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Stored),
        )
        .unwrap();
        zip.write_all(data).unwrap();
    }
    zip.finish().unwrap();
    path
}

fn validate(path: &Path, profile: RuntimeProfile, abis: &[&str]) -> anyhow::Result<()> {
    validate_runtime_resources(
        path,
        profile,
        EnvironmentPolicy::Strict,
        &abis.iter().map(|a| a.to_string()).collect::<Vec<_>>(),
    )
}

#[test]
fn 标准与工控模式严格匹配且不改文件() {
    for (api, profile, opposite) in [
        (21, RuntimeProfile::Standard, RuntimeProfile::AndroidApi19),
        (19, RuntimeProfile::AndroidApi19, RuntimeProfile::Standard),
    ] {
        let temp = tempfile::tempdir().unwrap();
        let path = archive(
            temp.path(),
            &META.replace(":21", &format!(":{api}")),
            &[],
            &[],
        );
        let before = fs::read(&path).unwrap();
        assert!(validate(&path, profile, &[]).is_ok());
        assert!(validate(&path, opposite, &[]).is_err());
        assert_eq!(before, fs::read(&path).unwrap());
        assert_eq!(fs::read_dir(temp.path()).unwrap().count(), 1);
    }
}

#[test]
fn 旧资源缺省标准且不要求发布版本一致() {
    for protocol in [2, 4] {
        let temp = tempfile::tempdir().unwrap();
        let text = META.replace("\"min_android_api\":21,", "").replace(
            "\"runtime_protocol\":4",
            &format!("\"runtime_protocol\":{protocol}"),
        );
        let path = archive(temp.path(), &text, &[], &[]);
        assert!(validate(&path, RuntimeProfile::Standard, &[]).is_ok());
        assert!(validate(&path, RuntimeProfile::AndroidApi19, &[]).is_err());
    }
}

#[test]
fn 拒绝不支持策略与内存候选() {
    let temp = tempfile::tempdir().unwrap();
    let path = archive(
        temp.path(),
        &META.replace(
            "\"environment_policy\":true",
            "\"environment_policy\":false",
        ),
        &[],
        &[],
    );
    assert!(validate(&path, RuntimeProfile::Standard, &[]).is_err());
    assert!(validate_runtime_resources(
        &path,
        RuntimeProfile::Standard,
        EnvironmentPolicy::Compatible,
        &[]
    )
    .is_ok());
    let memory = META
        .replace("\"runtime_protocol\":4", "\"runtime_protocol\":5")
        .replace(
            "\"memory_dex\":false",
            "\"memory_dex\":true,\"memory_dex_min_api\":31,\"stub_component_factory\":\"msk.f\"",
        );
    let path = archive(temp.path(), &memory, &[], &[]);
    assert!(validate(&path, RuntimeProfile::Standard, &[]).is_err());
}

#[test]
fn 拒绝未知歧义或伪造元数据声明() {
    let temp = tempfile::tempdir().unwrap();
    let variants = [
        META.replace("\"runtime_protocol\":4", "\"runtime_protocol\":99"),
        META.replace("\"runtime_protocol\":4", "\"runtime_protocol\":\"4\""),
        META.replace(
            "\"runtime_protocol\":4",
            "\"runtime_protocol\":4,\"runtime_protocol\":2",
        ),
        META.replace(
            "\"native_name_scheme\":1",
            "\"native_name_scheme\":1,\"native_name_scheme\":9",
        ),
        META.replace("\"cache_schema\":1", "\"cache_schema\":2"),
        META.replace("\"min_android_api\":21", "\"min_android_api\":20"),
        META.replace("\"min_android_api\":21", "\"min_android_api\":0"),
        META.replace("\"native_name_length\":16", "\"native_name_length\":15"),
        META.replace("\"native_name_length\":16", "\"native_name_length\":16.1"),
        META.replace("\"native_name_scheme\":1", "\"native_name_scheme\":1.1"),
        META.replace("\"native_name_scheme\":1", "\"native_name_scheme\":\"1\""),
        META.replace("libmocikashield.so", "libmocika\\u0073hield.so"),
        META.replace(
            "\"native_library\":",
            "\"nested\":{\"native_library\":\"other.so\"},\"native_library\":",
        ),
        META.replace(
            "\"runtime_protocol\":4,",
            "\"unknown\":{\"runtime_protocol\":4},",
        ),
        format!("{META} trailing"),
        format!("[{META}]"),
        META.trim_end_matches('}').to_owned(),
    ];
    for text in variants {
        let path = archive(temp.path(), &text, &[], &[]);
        assert!(
            validate(&path, RuntimeProfile::Standard, &[]).is_err(),
            "不应接受歧义元数据"
        );
    }
}

#[test]
fn 必需_stub_和规范库不可缺失或为空() {
    let temp = tempfile::tempdir().unwrap();
    for missing in [
        "metadata.json",
        "stub-classes.dex",
        "lib/arm64-v8a/libmocikashield.so",
    ] {
        let path = archive(temp.path(), META, &[missing], &[]);
        assert!(validate(&path, RuntimeProfile::Standard, &[]).is_err());
    }
    for name in ["stub-classes.dex", "lib/x86/libmocikashield.so"] {
        let path = archive(temp.path(), META, &[name], &[(name, b"")]);
        assert!(validate(&path, RuntimeProfile::Standard, &[]).is_err());
    }
}

#[test]
fn 空架构要求四种壳但有业务库时只要求保留集合() {
    let temp = tempfile::tempdir().unwrap();
    let path = archive(
        temp.path(),
        META,
        &["lib/arm64-v8a/libmocikashield.so"],
        &[],
    );
    assert!(validate(&path, RuntimeProfile::Standard, &[]).is_err());
    assert!(validate(&path, RuntimeProfile::Standard, &["armeabi-v7a"]).is_ok());
    for abis in [
        vec!["mips"],
        vec!["armeabi-v7a", "armeabi-v7a"],
        vec!["../x86"],
    ] {
        assert!(validate(&path, RuntimeProfile::Standard, &abis).is_err());
    }
    let path = archive(temp.path(), &META.replace(":21", ":19"), &[], &[]);
    assert!(validate(&path, RuntimeProfile::AndroidApi19, &["arm64-v8a"]).is_err());
    assert!(validate(
        &path,
        RuntimeProfile::AndroidApi19,
        &["armeabi-v7a", "arm64-v8a"]
    )
    .is_ok());
}

#[test]
fn 拒绝跨平台不安全路径及文件父目录冲突() {
    let temp = tempfile::tempdir().unwrap();
    for name in [
        "../outside",
        "/absolute",
        "C:/bad",
        "lib\\x86\\bad",
        "lib/./bad",
        "lib//bad",
        "lib/CON",
        "lib/name.",
    ] {
        let path = archive(temp.path(), META, &[], &[(name, b"bad")]);
        assert!(
            validate(&path, RuntimeProfile::Standard, &[]).is_err(),
            "不应接受异常资源路径"
        );
    }
    let path = archive(
        temp.path(),
        META,
        &[],
        &[("assets", b"file"), ("assets/child", b"bad")],
    );
    assert!(validate(&path, RuntimeProfile::Standard, &[]).is_err());
}

#[test]
fn 拒绝大小写和精确重复条目() {
    let temp = tempfile::tempdir().unwrap();
    let path = archive(temp.path(), META, &[], &[("Metadata.json", b"bad")]);
    assert!(validate(&path, RuntimeProfile::Standard, &[]).is_err());
    let path = archive(
        temp.path(),
        META,
        &[],
        &[("metadatz.json", META.as_bytes())],
    );
    let mut bytes = fs::read(&path).unwrap();
    let mut offset = 0;
    while offset + 13 <= bytes.len() {
        if &bytes[offset..offset + 13] == b"metadatz.json" {
            bytes[offset..offset + 13].copy_from_slice(b"metadata.json");
        }
        offset += 1;
    }
    fs::write(&path, bytes).unwrap();
    assert!(validate(&path, RuntimeProfile::Standard, &[]).is_err());
}

#[test]
fn 拒绝超限元数据与损坏内容校验和() {
    let temp = tempfile::tempdir().unwrap();
    let large = format!("{META}{}", " ".repeat(64 * 1024));
    let path = archive(temp.path(), &large, &[], &[]);
    assert!(validate(&path, RuntimeProfile::Standard, &[]).is_err());
    let path = archive(temp.path(), META, &[], &[]);
    let mut bytes = fs::read(&path).unwrap();
    let offset = {
        let mut zip = zip::ZipArchive::new(fs::File::open(&path).unwrap()).unwrap();
        let offset = zip.by_name("stub-classes.dex").unwrap().data_start() as usize;
        offset
    };
    bytes[offset] ^= 1;
    fs::write(&path, bytes).unwrap();
    assert!(validate(&path, RuntimeProfile::Standard, &[]).is_err());
}

#[test]
fn 拒绝符号链接和非规范附加资源() {
    let temp = tempfile::tempdir().unwrap();
    let path = archive(temp.path(), META, &[], &[]);
    let mut zip = zip::ZipWriter::new_append(
        fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&path)
            .unwrap(),
    )
    .unwrap();
    zip.add_symlink(
        "link",
        "stub-classes.dex",
        zip::write::SimpleFileOptions::default(),
    )
    .unwrap();
    zip.finish().unwrap();
    assert!(validate(&path, RuntimeProfile::Standard, &[]).is_err());
    for name in [
        "AndroidManifest.xml",
        "classes.dex",
        "lib/mips/libmocikashield.so",
        "lib/x86/extra.so",
    ] {
        let path = archive(temp.path(), META, &[], &[(name, b"unexpected")]);
        assert!(validate(&path, RuntimeProfile::Standard, &[]).is_err());
    }
}

#[test]
fn 资源读取上限在格式解析前检查且不支持注释布局() {
    let temp = tempfile::tempdir().unwrap();
    let names: Vec<_> = (0..256).map(|i| format!("extra/{i}")).collect();
    let extra: Vec<_> = names
        .iter()
        .map(|name| (name.as_str(), b"x".as_slice()))
        .collect();
    let path = archive(temp.path(), META, &[], &extra);
    assert!(validate(&path, RuntimeProfile::Standard, &[]).is_err());
    let path = archive(temp.path(), META, &[], &[]);
    let mut zip = zip::ZipWriter::new_append(
        fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&path)
            .unwrap(),
    )
    .unwrap();
    zip.set_comment("本轮只读准入不接受注释资源包");
    zip.finish().unwrap();
    assert!(validate(&path, RuntimeProfile::Standard, &[]).is_err());
}

#[test]
#[ignore = "显式核对本机已构建的标准、工控和内存候选资源，不重新构建资源"]
fn 本机资源正反模式只读核对() {
    let root =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../shield-stub/build/outputs/resources");
    for (name, profile, opposite) in [
        (
            "resources.zip",
            RuntimeProfile::Standard,
            RuntimeProfile::AndroidApi19,
        ),
        (
            "resources-api19.zip",
            RuntimeProfile::AndroidApi19,
            RuntimeProfile::Standard,
        ),
    ] {
        let path = root.join(name);
        let before = fs::read(&path).unwrap();
        assert!(validate(&path, profile, &[]).is_ok());
        assert!(validate(&path, opposite, &[]).is_err());
        assert_eq!(before, fs::read(path).unwrap());
    }
    assert!(validate(
        &root.join("resources-memory.zip"),
        RuntimeProfile::Standard,
        &[]
    )
    .is_err());
}

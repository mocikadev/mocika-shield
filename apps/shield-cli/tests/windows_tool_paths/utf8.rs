//! 无损传参隔离原型；不接入生产 API、不提交编译产物。
use super::utf8_transport::{frame, Probe};
use super::{bounded, SECRET};
use serde_json::{json, Value};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::Output,
};

const UNICODE_PASSWORD: &str = "隔离口令一二三456";

fn json_output(output: &Output) -> Value {
    assert!(output.status.success(), "隔离原型操作失败，未证明无损");
    serde_json::from_slice(&output.stdout).expect("原型必须只输出固定 JSON")
}

#[test]
#[ignore = "仅手动编译 Java 8 原型并验证有效和损坏帧"]
fn 标准输入帧保持中文及特殊字符并拒绝损坏数据() {
    let probe = Probe::new();
    let input = probe.root.join("中文 目录&引号\"");
    // Windows 文件名不允许引号，参数值与文件系统有效性分开验证。
    let input = if cfg!(windows) {
        probe.root.join("中文 目录&符号")
    } else {
        input
    };
    fs::create_dir(&input).unwrap();
    let path = input.join("内容😀.txt");
    fs::write(&path, "路径内容一致").unwrap();
    let output = probe.invoke(
        &[
            "roundtrip",
            "中文Release",
            "CN=中文主题",
            "空 格&引号\"反斜杠\\",
            "",
            "扩展😀",
            path.to_str().unwrap(),
        ],
        SECRET,
    );
    let result = json_output(&output);
    assert_eq!(result["arguments_match"], true);
    assert_eq!(result["unicode_file_read"], true);
    println!("无损参数原型：{result}");

    let valid = frame(&["unknown"]).unwrap();
    let mut wrong_magic = valid.clone();
    wrong_magic[0] ^= 1;
    let mut wrong_version = valid.clone();
    wrong_version[7] = 2;
    let mut too_many = valid.clone();
    too_many[11] = 65;
    let mut oversized = valid.clone();
    oversized[12..16].copy_from_slice(&65536u32.to_be_bytes());
    let mut invalid_utf8 = frame(&["x"]).unwrap();
    invalid_utf8[16] = 0xff;
    let mut trailing = valid.clone();
    trailing.push(0);
    let mut truncated = valid.clone();
    truncated.pop();
    for bytes in [
        wrong_magic,
        wrong_version,
        too_many,
        oversized,
        invalid_utf8,
        trailing,
        truncated,
        valid,
    ] {
        let output = probe.raw(&bytes, SECRET);
        assert_eq!(output.status.code(), Some(2), "非法帧或操作必须显式拒绝");
        assert!(output.stdout.is_empty());
    }
    println!("损坏帧拒绝：{}", json!({"rejected":8}));
}

fn ascii_subject(subject: &str) -> String {
    let mut escaped = String::new();
    for byte in subject.bytes() {
        if byte.is_ascii() {
            escaped.push(byte as char);
        } else {
            use std::fmt::Write;
            write!(&mut escaped, "\\{byte:02X}").unwrap();
        }
    }
    escaped
}

fn create_seed(probe: &Probe, kind: &str, password: &str) -> (PathBuf, String) {
    let source = probe.root.join(format!("seed-{kind}.keystore"));
    let mut command = shield_core::keytool::keytool_command(&probe.keytool);
    command
        .args([
            "-genkeypair",
            "-storetype",
            kind,
            "-alias",
            "release",
            "-storepass:env",
            "SHIELD_PATH_PROBE_PASS",
            "-keypass:env",
            "SHIELD_PATH_PROBE_PASS",
            "-dname",
            &ascii_subject("CN=中文主题"),
            "-keyalg",
            "RSA",
            "-keysize",
            "2048",
            "-validity",
            "7",
            "-keystore",
        ])
        .arg(&source)
        .env("SHIELD_PATH_PROBE_PASS", password)
        .env_remove("JAVA_TOOL_OPTIONS")
        .env_remove("_JAVA_OPTIONS")
        .env_remove("JDK_JAVA_OPTIONS");
    let output = bounded(command, &probe.root);
    assert!(output.status.success(), "keytool 合成证书生成失败");
    let fingerprint =
        super::text::reference_fingerprint(&probe.keytool, &probe.root, &source, password, kind);
    (source, fingerprint.replace(':', "").to_ascii_lowercase())
}

fn synthetic_manifest() -> Vec<u8> {
    // 仅签名夹具：有效的最小二进制 <manifest/>，没有应用、业务代码或安装承诺。
    // 字符串池只有 manifest；开始／结束元素无属性，不复用 GUI 解析或新增生产编码器。
    let mut bytes: Vec<u8> = [0x0008_0003u32, 112, 0x001c_0001, 44, 1, 0, 0x100, 32, 0, 0]
        .into_iter()
        .flat_map(u32::to_le_bytes)
        .collect();
    bytes.extend_from_slice(b"\x08\x08manifest\0\0");
    for words in [
        &[
            0x0010_0102u32,
            36,
            1,
            u32::MAX,
            u32::MAX,
            0,
            0x0014_0014,
            0,
            0,
        ][..],
        &[0x0010_0103u32, 24, 1, u32::MAX, u32::MAX, 0][..],
    ] {
        bytes.extend(words.iter().flat_map(|value| value.to_le_bytes()));
    }
    assert_eq!(bytes.len(), 112);
    bytes
}

fn signer_roundtrip(probe: &Probe, kind: &str, password: &str, store: &Path, fingerprint: &str) {
    let original_jar = shield_core::utils::find_apksigner().unwrap();
    let jar = store.parent().unwrap().join("签名 工具.jar");
    fs::copy(&original_jar, &jar).unwrap();
    let input = store.parent().unwrap().join("未签名 合成.apk");
    let output = store.parent().unwrap().join("已签名 合成.apk");
    let mut archive = zip::ZipWriter::new(fs::File::create(&input).unwrap());
    archive
        .start_file(
            "AndroidManifest.xml",
            zip::write::SimpleFileOptions::default(),
        )
        .unwrap();
    archive.write_all(&synthetic_manifest()).unwrap();
    archive
        .start_file("payload.txt", zip::write::SimpleFileOptions::default())
        .unwrap();
    archive.write_all(b"isolated-signing-fixture").unwrap();
    archive.finish().unwrap();
    let before = fs::read(&input).unwrap();
    let result = probe.invoke(
        &[
            "apksigner",
            jar.to_str().unwrap(),
            "sign",
            "--ks",
            store.to_str().unwrap(),
            "--ks-type",
            kind,
            "--ks-key-alias",
            "中文Release",
            "--ks-pass",
            "env:SHIELD_PATH_PROBE_PASS",
            "--key-pass",
            "env:SHIELD_PATH_PROBE_PASS",
            "--min-sdk-version",
            "21",
            "--v1-signing-enabled",
            "true",
            "--v2-signing-enabled",
            "true",
            "--v3-signing-enabled",
            "true",
            "--v4-signing-enabled",
            "false",
            "--out",
            output.to_str().unwrap(),
            input.to_str().unwrap(),
        ],
        password,
    );
    assert!(result.status.success(), "原始 apksigner 的中文材料签名失败");
    assert_eq!(fs::read(&input).unwrap(), before, "合成输入不得被修改");
    let verify = probe.root.join("verify.apk");
    fs::copy(&output, &verify).unwrap();
    // 不经参数助手，以原工具和 ASCII 文件独立验签及比较证书指纹。
    let mut command = shield_core::utils::java_command(&probe.java);
    command
        .arg("-jar")
        .arg(original_jar)
        .args([
            "verify",
            "--min-sdk-version",
            "21",
            "--verbose",
            "--print-certs",
        ])
        .arg(verify);
    let result = bounded(command, &probe.root);
    let diagnostic = format!(
        "{}{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(
        result.status.success(),
        "合成签名的独立原工具验签失败：{}",
        json!({
            "missing_manifest":diagnostic.contains("Missing AndroidManifest.xml"),
            "malformed_manifest":diagnostic.contains("Malformed AndroidManifest.xml"),
            "no_v1":diagnostic.contains("Missing META-INF/MANIFEST.MF"),
            "exit":result.status.code()
        })
    );
    assert!(
        String::from_utf8_lossy(&result.stdout).contains(fingerprint),
        "独立验签证书指纹不匹配"
    );
    let verified = String::from_utf8_lossy(&result.stdout);
    for scheme in ["v1", "v2", "v3"] {
        assert!(
            verified.lines().any(|line| line
                .starts_with(&format!("Verified using {scheme} scheme"))
                && line.ends_with(": true")),
            "独立验签未证明 {scheme} 有效"
        );
    }
    let rejected = probe.invoke(
        &["apksigner", jar.to_str().unwrap(), "--unknown-probe-option"],
        password,
    );
    assert!(!rejected.status.success(), "原型不得吞掉原工具的错误退出");
    println!(
        "中文签名原型：{}",
        json!({"kind":kind,"signed":true,"independently_verified":true,"v1_v2_v3_verified":true,
            "input_unchanged":true,"invalid_tool_option_rejected":true})
    );
}

#[test]
#[ignore = "仅手动验证 keytool、标准证书接口及仓库原始 JAR，不是 Android 安装验收"]
fn 标准证书接口及原始工具处理中文材料() {
    let probe = Probe::new();
    for (kind, password) in [("PKCS12", SECRET), ("JKS", UNICODE_PASSWORD)] {
        let (source, fingerprint) = create_seed(&probe, kind, password);
        let directory = probe.root.join(format!("中文 材料-{kind}"));
        fs::create_dir(&directory).unwrap();
        let store = directory.join("中文 证书.keystore");
        let failed = directory.join("错误口令不得生成.keystore");
        let rejected = probe.invoke(
            &[
                "certificate",
                source.to_str().unwrap(),
                failed.to_str().unwrap(),
                "中文Release",
                "CN=中文主题",
                kind,
                &fingerprint,
            ],
            "isolated-wrong-password",
        );
        assert_eq!(rejected.status.code(), Some(3), "错误口令必须显式失败");
        assert!(!failed.exists(), "错误口令不得生成目标证书");
        let result = json_output(&probe.invoke(
            &[
                "certificate",
                source.to_str().unwrap(),
                store.to_str().unwrap(),
                "中文Release",
                "CN=中文主题",
                kind,
                &fingerprint,
            ],
            password,
        ));
        for field in [
            "alias_matches",
            "subject_matches",
            "fingerprint_matches",
            "password_matches",
            "key_matches",
        ] {
            assert_eq!(result[field], true, "证书实际内容不匹配：{field}");
        }
        println!("无损证书原型：{}", json!({"kind":kind,"content":result}));
        let before = fs::read(&store).unwrap();
        let rejected = probe.invoke(
            &[
                "certificate",
                source.to_str().unwrap(),
                store.to_str().unwrap(),
                "中文Release",
                "CN=中文主题",
                kind,
                &fingerprint,
            ],
            password,
        );
        assert_eq!(rejected.status.code(), Some(3), "已有目标必须显式拒绝");
        assert_eq!(fs::read(&store).unwrap(), before, "不得截断或覆盖已有目标");
        println!(
            "证书负向原型：{}",
            json!({"kind":kind,"wrong_password_rejected":true,"existing_target_unchanged":true})
        );
        signer_roundtrip(&probe, kind, password, &store, &fingerprint);
    }
    let original = shield_core::utils::find_apktool().unwrap();
    let jar = probe.root.join("中文 资源工具.jar");
    fs::copy(&original, &jar).unwrap();
    let actual = probe.invoke(&["apktool", jar.to_str().unwrap(), "--version"], SECRET);
    let mut command = shield_core::utils::java_command(&probe.java);
    command.arg("-jar").arg(original).arg("--version");
    let baseline = bounded(command, &probe.root);
    assert!(baseline.status.success() && actual.status.success());
    assert_eq!(
        actual.stdout, baseline.stdout,
        "apktool 入口版本与原工具不匹配"
    );
    assert!(!actual.stdout.is_empty());
    println!(
        "资源工具原型：{}",
        json!({"unicode_jar_loaded":true,"version_matches":true,"full_decode_tested":false})
    );
}

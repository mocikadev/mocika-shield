//! 检查实际证书内容，不将 keytool 的成功退出等同于中文参数无损。
use super::{bounded, generate_with, Settings, SECRET};
use serde_json::{json, Value};
use shield_services::certificates::query_keystore_aliases;
use std::{fs, path::Path};

pub(super) fn reference_fingerprint(
    keytool: &Path,
    root: &Path,
    store: &Path,
    password: &str,
    kind: &str,
) -> String {
    // 参考材料的路径及 Alias 固定为 ASCII，直接执行原工具，不能随默认适配切换。
    assert!(store.as_os_str().is_ascii());
    let mut command = shield_core::keytool::keytool_command(keytool);
    command
        .args([
            "-list",
            "-v",
            "-alias",
            "release",
            "-storetype",
            kind,
            "-keystore",
        ])
        .arg(store)
        .args(["-storepass:env", "SHIELD_PATH_PROBE_PASS"])
        .env("SHIELD_PATH_PROBE_PASS", password);
    let output = bounded(command, root);
    assert!(output.status.success(), "独立 ASCII 原工具指纹基线失败");
    let text = std::str::from_utf8(&output.stdout).unwrap();
    text.lines()
        .find_map(|line| {
            let (label, value) = line.trim().split_once(':')?;
            let value = value.replace([':', ' '], "").to_ascii_uppercase();
            (label.trim() == "SHA256"
                && value.len() == 64
                && value.bytes().all(|byte| byte.is_ascii_hexdigit()))
            .then_some(value)
        })
        .expect("原工具未返回完整 SHA256 指纹")
}

fn inspect(
    root: &Path,
    java: &Path,
    store: &Path,
    alias: &str,
    subject: &str,
    rename_to: Option<&Path>,
) -> Value {
    // 期望内容不经命令参数传输，避免发送端和接收端发生相同损坏后误报匹配。
    fs::write(root.join("alias.txt"), alias).unwrap();
    fs::write(root.join("subject.txt"), subject).unwrap();
    let mut command = shield_core::utils::java_command(java);
    command
        .arg("-cp")
        .arg(root)
        .arg("CertificateContentProbe")
        .arg(store)
        .arg(root.join("alias.txt"))
        .arg(root.join("subject.txt"))
        .env("SHIELD_PATH_PROBE_PASS", SECRET)
        .env_remove("JAVA_TOOL_OPTIONS")
        .env_remove("_JAVA_OPTIONS")
        .env_remove("JDK_JAVA_OPTIONS");
    if let Some(target) = rename_to {
        command.arg(target);
    }
    let output = bounded(command, root);
    assert!(output.status.success(), "自有证书内容探针运行失败");
    serde_json::from_slice(&output.stdout).expect("自有证书探针必须只返回固定 JSON")
}

#[test]
#[ignore = "仅手动运行；分别核对中文参数生成的内容与独立构造的中文别名读取"]
fn 中文别名和主题必须检查实际证书内容() {
    let directory = tempfile::Builder::new()
        .prefix("shield-content-probe-")
        .tempdir()
        .unwrap();
    let root = shield_core::utils::strip_unc_prefix(directory.path());
    assert!(root.as_os_str().is_ascii(), "此探针要求独立的 ASCII 临时根");
    let keytool = shield_core::utils::find_keytool().unwrap();
    // 使用 keytool 同一安装中的 Java／javac，避免跨 JDK 混合取得证据。
    let bin = keytool.parent().unwrap();
    let java = bin.join(format!("java{}", std::env::consts::EXE_SUFFIX));
    let source = root.join("CertificateContentProbe.java");
    fs::write(
        &source,
        include_str!("../../../../tests/fixtures/agent-signing/CertificateContentProbe.java"),
    )
    .unwrap();
    let mut compile = shield_core::utils::no_window_command(
        bin.join(format!("javac{}", std::env::consts::EXE_SUFFIX)),
    );
    compile
        .args(["-encoding", "UTF-8", "-source", "8", "-target", "8", "-d"])
        .arg(&root)
        .arg(source);
    assert!(
        bounded(compile, &root).status.success(),
        "自有证书探针编译失败"
    );
    let mut ascii_store = None;
    for settings in [
        Settings::Project,
        Settings::Plain,
        Settings::NoFileEncoding,
        Settings::NoLocale,
    ] {
        for (case, alias, subject) in [
            ("ascii", "release", "CN=path-fixture"),
            ("alias", "中文Release", "CN=path-fixture"),
            ("subject", "release", "CN=中文主题"),
        ] {
            let target = root.join(format!("{}-{case}.p12", settings.name()));
            let generation = generate_with(&keytool, &target, alias, subject, &root, settings);
            let mut result =
                json!({"case":case, "settings":settings.name(), "generation":generation});
            if result["generation"]["succeeded"] == true {
                result["content"] = inspect(&root, &java, &target, alias, subject, None);
            }
            if case == "ascii" {
                assert_eq!(result["generation"]["created"], true, "ASCII 证书基线失败");
                assert_eq!(result["content"]["alias_matches"], true);
                assert_eq!(result["content"]["subject_matches"], true);
                ascii_store = Some(target);
            }
            println!("证书内容对照：{result}");
        }
    }

    // 标准 KeyStore 接口从 UTF-8 文件取得中文 Alias，独立生成已知正确的读取夹具。
    // 不用损坏后的 keytool 参数创建夹具，否则读写两次相同损坏可能掩盖错误。
    let source = ascii_store.unwrap();
    let fixture = root.join("independent-alias.p12");
    let renamed = inspect(
        &root,
        &java,
        &source,
        "中文Release",
        "CN=path-fixture",
        Some(&fixture),
    );
    assert_eq!(renamed["alias_matches"], true, "独立中文别名夹具生成失败");
    assert_eq!(renamed["subject_matches"], true);
    let expected_fingerprint = reference_fingerprint(&keytool, &root, &source, SECRET, "PKCS12");
    let aliases = query_keystore_aliases(
        fixture.to_str().unwrap().into(),
        SECRET.into(),
        Some("PKCS12".into()),
    );
    let fingerprint = shield_core::apk_inspect::extract_keystore_cert_fingerprint(
        &fixture,
        "中文Release",
        SECRET,
        Some("PKCS12"),
    );
    println!(
        "独立中文别名读取：{}",
        json!({
            "fixture_alias_matches":renamed["alias_matches"],
            "query_succeeded":aliases.is_ok(),
            "query_alias_matches":aliases.as_ref().is_ok_and(|values| values.len() == 1 && values[0] == "中文release"),
            "fingerprint_succeeded":fingerprint.is_ok(),
            "fingerprint_matches":fingerprint.as_ref().is_ok_and(|value| value == &expected_fingerprint)
        })
    );
}

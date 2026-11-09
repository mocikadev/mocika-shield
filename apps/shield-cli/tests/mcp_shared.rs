//! MCP 共享来源的协议、权限与真实执行验收；只操作自有隔离资料。
#![cfg(all(feature = "mcp-server", target_os = "macos"))]
use serde_json::{json, Value};
use shield_app::{
    certificates::{CertificatePreferencesPatch, CertificateStoreState, CertificateUpsertInput},
    config::ConfigStore,
    paths::ApplicationPaths,
};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};
#[path = "support/mcp_client.rs"]
mod client;
#[path = "support/execution_fixture.rs"]
mod real_fixture;
use client::Session;

fn draft(name: &str) -> CertificateUpsertInput {
    CertificateUpsertInput {
        id: None,
        name: name.into(),
        source_type: "external".into(),
        keystore_path: String::new(),
        keystore_password: real_fixture::Fixture::password().into(),
        key_alias: "agentfixture".into(),
        key_password: String::new(),
        ks_type: Some("PKCS12".into()),
        sign_v1: true,
        sign_v2: true,
        sign_v3: true,
        sign_v4: false,
        auto_sign_enabled: true,
        note: "private-note".into(),
        set_as_default: true,
        copy_keystore_to_managed: false,
        managed_file_name: None,
    }
}
fn command(root: &Path, home: &Path, ids: &[&str], execute: bool) -> Command {
    configured(Command::new(program()), root, home, ids, execute)
}
fn program() -> PathBuf {
    let bundle = std::env::var_os("SHIELD_SHARED_TEST_APP").map(PathBuf::from);
    bundle
        .map(|app| {
            assert!(app.is_absolute() && app.is_dir());
            app.join("Contents/MacOS/shield")
        })
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_BIN_EXE_shield")))
}
fn configured(
    mut command: Command,
    root: &Path,
    home: &Path,
    ids: &[&str],
    execute: bool,
) -> Command {
    command
        .arg("--app-home")
        .arg(home)
        .args(["mcp", "serve", "--use-shared-settings", "--input-root"])
        .arg(root.join("输入"))
        .arg("--output-root")
        .arg(root.join("输出"))
        .args(["--timeout-seconds", "120"])
        .env("MOCIKA_SHIELD_KS_PASS", "inherited-must-not-be-used")
        .env("MOCIKA_SHIELD_KEY_PASS", "inherited-key-must-not-be-used")
        .env_remove("JAVA_TOOL_OPTIONS")
        .env_remove("_JAVA_OPTIONS")
        .env_remove("JDK_JAVA_OPTIONS");
    for id in ids {
        command.arg(format!("--allow-certificate={id}"));
    }
    if execute {
        command.arg("--allow-execution");
    }
    command
}
fn directories(root: &Path) {
    for name in ["输入", "输出"] {
        fs::create_dir(root.join(name)).unwrap();
    }
}
struct TestProgram {
    cli: PathBuf,
    files: Vec<(PathBuf, [u8; 32])>,
}
impl TestProgram {
    fn new(real: &real_fixture::Fixture) -> Self {
        let source = program();
        let mut files = Vec::new();
        let cli = if std::env::var_os("SHIELD_SHARED_TEST_APP").is_some() {
            let source_app = source.parent().unwrap().parent().unwrap().parent().unwrap();
            let app = real.root().join("独立 应用.app");
            assert!(!app.exists());
            assert!(Command::new("ditto")
                .arg(source_app)
                .arg(&app)
                .status()
                .unwrap()
                .success());
            assert!(!app.join("Contents/Resources/skills").exists());
            for name in [
                "Contents/MacOS/shield",
                "Contents/MacOS/mocika-shield",
                "Contents/Resources/tools/apktool.jar",
                "Contents/Resources/tools/apksigner.jar",
                "Contents/Resources/resources/resources.zip",
                "Contents/Resources/resources/resources-api19.zip",
                "Contents/_CodeSignature/CodeResources",
            ] {
                let path = app.join(name);
                let hash = real_fixture::hash(&path);
                files.push((path, hash));
            }
            app.join("Contents/MacOS/shield")
        } else {
            source
        };
        // 技能独立安装在测试工程，不放进应用；实际定位当前要启动的同一个 CLI。
        let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let installed = real
            .root()
            .join("临时 Android 工程/.agents/skills/mocika-shield");
        for name in [
            "SKILL.md",
            "agents/openai.yaml",
            "references/cli.md",
            "references/shared-data.md",
            "scripts/resolve-cli.sh",
        ] {
            let target = installed.join(name);
            fs::create_dir_all(target.parent().unwrap()).unwrap();
            fs::copy(repo.join("skills/mocika-shield").join(name), target).unwrap();
        }
        let located = Command::new("/bin/bash")
            .arg(installed.join("scripts/resolve-cli.sh"))
            .arg(&cli)
            .current_dir(real.root())
            .env_remove("BASH_ENV")
            .output()
            .unwrap();
        assert!(located.status.success());
        assert!(located.stderr.is_empty());
        let resolved = PathBuf::from(
            String::from_utf8(located.stdout)
                .unwrap()
                .trim_end_matches('\n'),
        );
        // 定位器会解析 macOS 的 /var 别名；这里只比对可执行文件，不改密文目录上下文。
        assert_eq!(
            resolved.canonicalize().unwrap(),
            cli.canonicalize().unwrap()
        );
        Self {
            cli: resolved,
            files,
        }
    }
    fn unchanged(&self) {
        for (path, hash) in &self.files {
            assert_eq!(&real_fixture::hash(path), hash, "任务不得改写应用包");
        }
        if !self.files.is_empty() {
            let app = self
                .cli
                .parent()
                .unwrap()
                .parent()
                .unwrap()
                .parent()
                .unwrap();
            assert!(Command::new("codesign")
                .args(["--verify", "--deep", "--strict"])
                .arg(app)
                .status()
                .unwrap()
                .success());
        }
    }
}
fn assert_private(value: &Value, root: &Path) {
    let text = value.to_string();
    for secret in [
        real_fixture::Fixture::password(),
        "inherited-must-not-be-used",
        "inherited-key-must-not-be-used",
        "private-name",
        "private-note",
        "agentfixture",
        root.to_str().unwrap(),
        "shared_binding",
    ] {
        assert!(!text.contains(secret), "结果不应包含私有内容");
    }
}

#[test]
fn 共享查询只见白名单且没有写权限或任意资料入口() {
    let root = tempfile::tempdir().unwrap();
    directories(root.path());
    fs::write(root.path().join("输入/a.apk"), b"fixture").unwrap();
    let home = root.path().join("共享资料");
    let paths = ApplicationPaths::isolated(&home).unwrap();
    let store =
        CertificateStoreState::initialize(paths.certificate_database(), paths.keystore_dir())
            .unwrap();
    let first = store
        .save_certificate(
            &draft("private-name"),
            "private-keystore",
            "agentfixture",
            None,
        )
        .unwrap();
    let hidden = store
        .save_certificate(
            &draft("hidden-name"),
            "hidden-keystore",
            "hidden-alias",
            None,
        )
        .unwrap();
    let before = fs::read(paths.certificate_database()).unwrap();
    let mut session = Session::start(command(root.path(), &home, &[&first.id], false));
    session.send(json!({"jsonrpc":"2.0","id":2,"method":"tools/list"}));
    let tools = session.receive(2)["result"]["tools"]
        .as_array()
        .unwrap()
        .clone();
    assert_eq!(tools.len(), 4);
    assert!(tools
        .iter()
        .all(|tool| tool["annotations"]["readOnlyHint"] == true));
    let result = session.call(3, "shield_certificates", json!({}));
    assert_eq!(result["status"], "ready");
    assert_eq!(result["data"]["certificates"].as_array().unwrap().len(), 1);
    assert_eq!(result["data"]["certificates"][0]["id"], first.id);
    assert!(!result.to_string().contains(&hidden.id));
    assert_private(&result, root.path());
    assert_eq!(
        session.call(
            4,
            "shield_certificates",
            json!({"certificate_id":hidden.id})
        )["code"],
        "CERTIFICATE_NOT_ALLOWED"
    );
    assert_eq!(
        session.call(5, "shield_certificates", json!({"database":"not-allowed"}))["code"],
        "ARGUMENT_INVALID"
    );
    assert_eq!(
        session.call(6, "shield_plan", json!({"input":"a.apk","output":"b.apk"}))["code"],
        "CERTIFICATE_REQUIRED"
    );
    assert_eq!(
        session.call(
            7,
            "shield_plan",
            json!({"input":"a.apk","output":"b.apk","certificate_id":hidden.id})
        )["code"],
        "CERTIFICATE_NOT_ALLOWED"
    );
    for name in [
        "shield_protect",
        "shield_sign",
        "shield_cert_import",
        "shield_cert_remove",
    ] {
        assert_eq!(
            session.call(8, name, json!({}))["code"],
            "TOOL_NOT_AVAILABLE"
        );
    }
    session.close();
    assert_eq!(fs::read(paths.certificate_database()).unwrap(), before);
    assert!(!root.path().join("输出/b.apk").exists());
}

#[test]
fn 空库查询不创建资料且启动范围不能覆盖资料目录() {
    let root = tempfile::tempdir().unwrap();
    directories(root.path());
    let home = root.path().join("缺失上下文");
    let mut session = Session::start(command(root.path(), &home, &["allowed"], false));
    let result = session.call(2, "shield_certificates", json!({}));
    assert_eq!(result["status"], "ready");
    assert_eq!(result["data"]["certificates"], json!([]));
    session.close();
    assert!(!home.exists());
    for (context, ids) in [
        (&home, vec![]),
        (&root.path().join("输出"), vec!["allowed"]),
    ] {
        let output = command(root.path(), context, &ids, false).output().unwrap();
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
        assert!(!String::from_utf8_lossy(&output.stderr).contains(root.path().to_str().unwrap()));
    }
}

fn plan(session: &mut Session, id: u64, certificate: &str, output: &str) -> Value {
    let value = session.call(
        id,
        "shield_plan",
        json!({"input":"样本.apk","output":output,"certificate_id":certificate}),
    );
    assert!(
        matches!(value["status"].as_str(), Some("ready" | "warning")),
        "有限计划结果：{value}"
    );
    assert_eq!(value["plan"]["schema_version"], 3);
    assert!(value["plan"].get("shared_binding").is_none());
    value
}
fn reference(plan: &Value) -> Value {
    json!({"plan_id":plan["plan_id"],"expect_plan_sha256":plan["plan"]["sha256"]})
}

#[test]
#[ignore = "需要自有 APK、真实 JDK 与资源，验证共享会话执行取消／断开，不纳入日常 CI"]
fn 真实共享执行取消和断开均不交付或遗留工作材料() {
    use std::time::{Duration, Instant};
    for disconnect in [false, true] {
        let real = real_fixture::Fixture::new();
        let program = TestProgram::new(&real);
        directories(real.root());
        fs::copy(&real.input, real.root().join("输入/样本.apk")).unwrap();
        let home = real.root().join("共享取消资料");
        let paths = ApplicationPaths::isolated(&home).unwrap();
        let store =
            CertificateStoreState::initialize(paths.certificate_database(), paths.keystore_dir())
                .unwrap();
        let record = store
            .save_certificate(
                &draft("private-name"),
                real.root().join("临时测试证书.p12").to_str().unwrap(),
                "agentfixture",
                None,
            )
            .unwrap();
        let before = fs::read(paths.certificate_database()).unwrap();
        let mut session = Session::start(configured(
            real.command(&program.cli),
            real.root(),
            &home,
            &[&record.id],
            true,
        ));
        let planned = plan(&mut session, 2, &record.id, "取消不得出现.apk");
        session.send(json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{
            "name":"shield_protect","arguments":reference(&planned),"_meta":{"progressToken":"共享取消"}}}));
        session.wait_stage("unpack");
        if disconnect {
            session.close();
        } else {
            session.send(json!({"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":3,"reason":"测试取消"}}));
            let deadline = Instant::now() + Duration::from_secs(25);
            loop {
                let status = session.call(
                    4,
                    "shield_execution_status",
                    json!({"plan_id":planned["plan_id"]}),
                );
                assert_private(&status, real.root());
                if status["state"] == "finished" {
                    assert_eq!(status["status"], "cancelled");
                    assert_eq!(status["cleanup_state"], "complete");
                    break;
                }
                assert!(Instant::now() < deadline, "共享取消未形成终态");
                std::thread::sleep(Duration::from_millis(30));
            }
            session.close();
        }
        assert!(!real.root().join("输出/取消不得出现.apk").exists());
        for name in ["输入", "输出"] {
            assert!(!fs::read_dir(real.root().join(name))
                .unwrap()
                .any(|entry| entry
                    .unwrap()
                    .file_name()
                    .to_string_lossy()
                    .starts_with(".mocika-")));
        }
        assert_eq!(fs::read(paths.certificate_database()).unwrap(), before);
        assert!(!real.output.exists());
        real.assert_inputs_unchanged();
        real.assert_no_tasks();
        program.unchanged();
    }
}

#[test]
#[ignore = "需要自有 SHIELD_AGENT_TEST_APK、SHIELD_AGENT_TEST_ZIPALIGN 和真实 JDK；完整包可由 SHIELD_SHARED_TEST_APP 指定"]
fn 真实共享协议双模式签名一次消费与材料阻断() {
    let real = real_fixture::Fixture::new();
    let program = TestProgram::new(&real);
    directories(real.root());
    let zipalign = std::env::var_os("SHIELD_AGENT_TEST_ZIPALIGN").expect("指定 SDK zipalign");
    let config_hash = real_fixture::hash(&real.config);
    fs::copy(&real.input, real.root().join("输入/样本.apk")).unwrap();
    let home = real.root().join("共享应用资料");
    let paths = ApplicationPaths::isolated(&home).unwrap();
    let store =
        CertificateStoreState::initialize(paths.certificate_database(), paths.keystore_dir())
            .unwrap();
    let keystore = real.root().join("临时测试证书.p12");
    let ks_hash = real_fixture::hash(&keystore);
    let cert = store
        .save_certificate(
            &draft("private-name"),
            keystore.to_str().unwrap(),
            "agentfixture",
            None,
        )
        .unwrap();
    let config = ConfigStore::new(paths.config_file());
    config
        .update_protect_defaults(
            serde_json::from_value(
                json!({"runtime_mode":"android_api19","environment_policy":"compatible"}),
            )
            .unwrap(),
        )
        .unwrap();
    let expected =
        shield_core::extract_apk_cert_fingerprint(&real.input, Some(&real.signer)).unwrap();
    let mut session = Session::start(configured(
        real.command(&program.cli),
        real.root(),
        &home,
        &[&cert.id],
        true,
    ));
    let api19 = plan(&mut session, 2, &cert.id, "工控.apk");
    assert_private(&api19, real.root());
    config
        .update_protect_defaults(
            serde_json::from_value(json!({"runtime_mode":"standard"})).unwrap(),
        )
        .unwrap();
    store
        .patch_certificate_preferences(
            &cert.id,
            CertificatePreferencesPatch {
                name: Some("改名不换材料".into()),
                ..Default::default()
            },
            false,
        )
        .unwrap();
    let unused = real.root().join("不会使用.p12");
    fs::write(&unused, b"not-keystore").unwrap();
    store
        .save_certificate(&draft("另一默认"), unused.to_str().unwrap(), "unused", None)
        .unwrap();
    let mut wrong = reference(&api19);
    wrong["expect_plan_sha256"] = json!("0".repeat(64));
    assert_eq!(
        session.call(3, "shield_protect", wrong)["code"],
        "PLAN_DIGEST_MISMATCH"
    );
    for (id, planned, name) in [
        (4, api19, "工控.apk"),
        (7, plan(&mut session, 6, &cert.id, "标准.apk"), "标准.apk"),
    ] {
        let value = session.call(id, "shield_protect", reference(&planned));
        assert_eq!(value["status"], "succeeded", "有限执行结果：{value}");
        assert_private(&value, real.root());
        let status = session.call(
            id + 20,
            "shield_execution_status",
            json!({"plan_id":planned["plan_id"]}),
        );
        assert_eq!(status["state"], "finished");
        assert_private(&status, real.root());
        assert_eq!(
            session.call(id + 30, "shield_protect", reference(&planned))["code"],
            "PLAN_ALREADY_CONSUMED"
        );
        let output = real.root().join("输出").join(name);
        assert_eq!(
            shield_core::normalize_fingerprint(
                &shield_core::extract_apk_cert_fingerprint(&output, Some(&real.signer)).unwrap()
            ),
            shield_core::normalize_fingerprint(&expected)
        );
        assert!(shield_core::utils::no_window_command(&zipalign)
            .args(["-c", "-P", "16", "4"])
            .arg(output)
            .output()
            .unwrap()
            .status
            .success());
    }
    let blocked = plan(&mut session, 40, &cert.id, "删除后不得交付.apk");
    store.delete_certificate(&cert.id, false).unwrap();
    let value = session.call(41, "shield_protect", reference(&blocked));
    assert_eq!(value["code"], "PLAN_SOURCE_CHANGED");
    assert_private(&value, real.root());
    assert!(!real.root().join("输出/删除后不得交付.apk").exists());
    assert_eq!(
        session.call(42, "shield_protect", reference(&blocked))["code"],
        "PLAN_ALREADY_CONSUMED"
    );
    session.close();
    assert_eq!(real_fixture::hash(&keystore), ks_hash);
    assert_eq!(real_fixture::hash(&real.config), config_hash);
    real.assert_inputs_unchanged();
    assert!(!real.output.exists(), "不能写到未授权的默认输出");
    real.assert_no_tasks();
    program.unchanged();
}

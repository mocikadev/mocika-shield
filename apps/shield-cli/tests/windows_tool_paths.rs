//! 原生路径的单因素对照；仅合成证书，不访问真实资料或修改系统环境。
#[path = "windows_tool_paths/text.rs"]
mod text;
#[path = "windows_tool_paths/utf8.rs"]
mod utf8;
#[path = "windows_tool_paths/utf8_transport.rs"]
mod utf8_transport;

use serde_json::{json, Value};
use shield_services::{
    certificates::{
        create_managed_certificate, CertificateStoreState, CreateManagedCertificateInput,
    },
    paths::ApplicationPaths,
};
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
    time::{Duration, Instant},
};

const SECRET: &str = "isolated-path-probe-password";

#[derive(Clone, Copy)]
enum Settings {
    Project,
    Plain,
    NoFileEncoding,
    NoLocale,
}
impl Settings {
    fn name(self) -> &'static str {
        match self {
            Self::Project => "project",
            Self::Plain => "plain",
            Self::NoFileEncoding => "no_file_encoding",
            Self::NoLocale => "no_locale",
        }
    }
    fn command(self, seed: Command) -> Command {
        let mut command = shield_core::utils::no_window_command(seed.get_program());
        command.args(seed.get_args().filter(|value| {
            let value = value.to_string_lossy();
            match self {
                Self::Project => true,
                Self::Plain => false,
                Self::NoFileEncoding => !value.contains("-Dfile.encoding="),
                Self::NoLocale => {
                    !value.contains("-Duser.language=") && !value.contains("-Duser.country=")
                }
            }
        }));
        command
    }
}

fn extended(path: &Path) -> bool {
    #[cfg(windows)]
    {
        use std::path::{Component, Prefix};
        matches!(path.components().next(), Some(Component::Prefix(prefix))
            if matches!(prefix.kind(), Prefix::Verbatim(_) | Prefix::VerbatimUNC(_, _) | Prefix::VerbatimDisk(_)))
    }
    #[cfg(not(windows))]
    {
        let _ = path;
        false
    }
}

fn bounded(mut command: Command, capture: &Path) -> Output {
    command
        .stdout(fs::File::create(capture.join("stdout")).unwrap())
        .stderr(fs::File::create(capture.join("stderr")).unwrap());
    let mut child = command.spawn().unwrap();
    let until = Instant::now() + Duration::from_secs(30);
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if Instant::now() >= until {
            let _ = child.kill();
            let _ = child.wait();
            panic!("自有路径探针超时，不能判定工具兼容");
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    let read = |name| {
        let path = capture.join(name);
        assert!(path.metadata().unwrap().len() <= 65536, "自有工具输出超限");
        fs::read(path).unwrap()
    };
    Output {
        status,
        stdout: read("stdout"),
        stderr: read("stderr"),
    }
}

fn generate(keytool: &Path, target: &Path, alias: &str, subject: &str, capture: &Path) -> Value {
    generate_with(keytool, target, alias, subject, capture, Settings::Project)
}
fn generate_with(
    keytool: &Path,
    target: &Path,
    alias: &str,
    subject: &str,
    capture: &Path,
    settings: Settings,
) -> Value {
    assert!(!target.exists(), "自有目标必须尚不存在");
    let mut command = settings.command(shield_core::keytool::keytool_command(keytool));
    command
        .args([
            "-genkeypair",
            "-storetype",
            "PKCS12",
            "-alias",
            alias,
            "-storepass:env",
            "SHIELD_PATH_PROBE_PASS",
            "-keypass:env",
            "SHIELD_PATH_PROBE_PASS",
            "-dname",
            subject,
            "-keyalg",
            "RSA",
            "-keysize",
            "2048",
            "-validity",
            "7",
            "-keystore",
        ])
        .arg(target)
        .env("SHIELD_PATH_PROBE_PASS", SECRET)
        .env_remove("JAVA_TOOL_OPTIONS")
        .env_remove("_JAVA_OPTIONS")
        .env_remove("JDK_JAVA_OPTIONS");
    let output = bounded(command, capture);
    let text = format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    // 只输出固定分类与布尔值；原文、目录和口令不进入 CI 日志。
    json!({"succeeded":output.status.success(), "exit":output.status.code(),
        "bad_path":text.contains("Bad pathname"), "illegal_option":text.contains("Illegal option"),
        "target_extended":extended(target), "created":target.metadata().is_ok_and(|m| m.len() > 0)})
}

fn argument_controls(root: &Path, keytool: &Path) {
    let root = shield_core::utils::strip_unc_prefix(root);
    let java = shield_core::utils::find_java().unwrap();
    let javac = keytool
        .parent()
        .unwrap()
        .join(format!("javac{}", std::env::consts::EXE_SUFFIX));
    let source = root.join("ToolPathProbe.java");
    fs::write(
        &source,
        include_str!("../../../tests/fixtures/agent-signing/ToolPathProbe.java"),
    )
    .unwrap();
    let mut compile = shield_core::utils::no_window_command(javac);
    compile
        .args(["-encoding", "UTF-8", "-d"])
        .arg(&root)
        .arg(source);
    assert!(
        bounded(compile, &root).status.success(),
        "自有参数往返源码编译失败"
    );
    let dir = root.join("中文 参数");
    fs::create_dir(&dir).unwrap();
    for settings in [
        Settings::Project,
        Settings::Plain,
        Settings::NoFileEncoding,
        Settings::NoLocale,
    ] {
        let target = dir.join(format!("{}.p12", settings.name()));
        let expected = root.join("expected.txt");
        fs::write(&expected, target.to_str().unwrap()).unwrap();
        let mut command = settings.command(shield_core::utils::java_command(&java));
        command
            .arg("-cp")
            .arg(&root)
            .arg("ToolPathProbe")
            .arg(&target)
            .arg(&expected)
            .env_remove("JAVA_TOOL_OPTIONS")
            .env_remove("_JAVA_OPTIONS")
            .env_remove("JDK_JAVA_OPTIONS");
        let output = bounded(command, &root);
        assert!(output.status.success(), "自有参数往返工具失败");
        let mut value: Value =
            serde_json::from_slice(&output.stdout).expect("自有参数探针必须只返回固定 JSON");
        value["settings"] = json!(settings.name());
        value["keytool"] = generate_with(
            keytool,
            &target,
            "release",
            "CN=path-fixture",
            &root,
            settings,
        );
        println!("参数对照：{value}");
    }
}

fn draft() -> CreateManagedCertificateInput {
    CreateManagedCertificateInput {
        name: "路径对照".into(),
        file_name: "release".into(),
        key_alias: "release".into(),
        keystore_password: SECRET.into(),
        key_password: String::new(),
        ks_type: Some("PKCS12".into()),
        sign_v1: true,
        sign_v2: true,
        sign_v3: true,
        sign_v4: false,
        auto_sign_enabled: false,
        note: String::new(),
        set_as_default: true,
        dname: "CN=path-fixture".into(),
        validity_days: 7,
        key_size: 2048,
    }
}

#[test]
#[ignore = "仅手动运行的系统路径对照；诊断完成不代表各路径均受支持"]
fn 字符集与扩展前缀及共享证书创建的原生对照() {
    let root = tempfile::Builder::new()
        .prefix("shield-path-probe-")
        .tempdir()
        .unwrap();
    let keytool = shield_core::utils::find_keytool().unwrap();
    println!(
        "探针环境：{}",
        json!({
            "temporary_root_extended":extended(root.path()),
            "temporary_root_ascii":shield_core::utils::strip_unc_prefix(root.path()).as_os_str().is_ascii(),
            "keytool_program_extended":extended(&keytool),
            "external_java_options_present":(["JAVA_TOOL_OPTIONS", "_JAVA_OPTIONS", "JDK_JAVA_OPTIONS"]
                .iter().any(|name| std::env::var_os(name).is_some_and(|v| !v.is_empty())))
        })
    );
    let mut results = Vec::new();
    for chinese_dir in [false, true] {
        let directory = root.path().join(if chinese_dir {
            "中文 目录"
        } else {
            "ascii directory"
        });
        fs::create_dir(&directory).unwrap();
        for chinese_file in [false, true] {
            for canonical in [false, true] {
                let parent = if canonical {
                    fs::canonicalize(&directory).unwrap()
                } else {
                    shield_core::utils::strip_unc_prefix(&directory)
                };
                let name = if chinese_file { "证书" } else { "release" };
                let target = parent.join(format!(
                    "{name}-{}.p12",
                    if canonical { "canonical" } else { "normal" }
                ));
                let mut value =
                    generate(&keytool, &target, "release", "CN=path-fixture", root.path());
                value["case"] = json!("direct");
                value["chinese_directory"] = json!(chinese_dir);
                value["chinese_file"] = json!(chinese_file);
                value["canonical"] = json!(canonical);
                println!("路径对照：{value}");
                results.push(value);
            }
        }
        // 同一应用服务必须先自行规范化上下文；这里不在调用前替它改路径。
        for canonical in [false, true] {
            let home: PathBuf = if canonical {
                fs::canonicalize(&directory).unwrap()
            } else {
                shield_core::utils::strip_unc_prefix(&directory)
            };
            let home = home.join(if canonical {
                "home-canonical"
            } else {
                "home-normal"
            });
            let paths = ApplicationPaths::isolated(&home).unwrap();
            let store = CertificateStoreState::initialize(
                paths.certificate_database(),
                paths.keystore_dir(),
            )
            .unwrap();
            let outcome = create_managed_certificate(&store, draft());
            let mut value = json!({"case":"shared", "chinese_directory":chinese_dir, "canonical":canonical,
                "home_extended":extended(&home), "store_extended":extended(store.keystore_dir()), "succeeded":outcome.is_ok()});
            if let Err(error) = outcome {
                value["code"] = json!(error.diagnostic.code.as_str());
                value["exit"] = json!(error.diagnostic.exit_code);
                value["evidence"] = json!(error.diagnostic.evidence);
            }
            println!("路径对照：{value}");
            results.push(value);
        }
    }
    // 补 ASCII 路径上的文本参数控制，不把路径问题与 Alias／主题混为一谈。
    for (name, alias, subject) in [
        ("alias", "中文Release", "CN=path-fixture"),
        ("subject", "release", "CN=中文主题"),
    ] {
        let target = shield_core::utils::strip_unc_prefix(root.path()).join(format!("{name}.p12"));
        let mut value = generate(&keytool, &target, alias, subject, root.path());
        value["case"] = json!(name);
        println!("路径对照：{value}");
        results.push(value);
    }
    assert_eq!(results.len(), 14);
    let baseline = &results[0];
    assert_eq!(
        baseline["succeeded"], true,
        "普通 ASCII 基线不通过时，不能作字符集对照"
    );
    assert_eq!(baseline["created"], true);
    argument_controls(root.path(), &keytool);
}

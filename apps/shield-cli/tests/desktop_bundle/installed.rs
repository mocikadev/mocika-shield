//! 显式运行真实安装包内 CLI；测试资料独立，不使用项目工具覆盖或用户数据库。
use super::*;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::io::Read;
use std::path::PathBuf;
use std::process::Command;
#[path = "../support/mcp_client.rs"]
mod client;

fn snapshot(binary: &Path) -> Vec<[u8; 32]> {
    #[cfg(target_os = "macos")]
    let resources = binary.parent().unwrap().parent().unwrap().join("Resources");
    #[cfg(windows)]
    let resources = binary.parent().unwrap().to_path_buf();
    #[cfg(target_os = "linux")]
    let resources = binary.parent().unwrap().join("../lib/MocikaShield");
    let mut files = vec![
        binary.to_path_buf(),
        binary.with_file_name(if cfg!(windows) {
            "mocika-shield.exe"
        } else {
            "mocika-shield"
        }),
    ];
    files.extend(
        [
            "tools/apktool.jar",
            "tools/apksigner.jar",
            "tools/java-adapter.jar",
            "resources/resources.zip",
            "resources/resources-api19.zip",
        ]
        .iter()
        .map(|name| resources.join(name)),
    );
    files
        .iter()
        .map(|path| {
            let mut source = fs::File::open(path).unwrap();
            let mut digest = Sha256::new();
            let mut block = [0u8; 65536];
            loop {
                let count = source.read(&mut block).unwrap();
                if count == 0 {
                    break;
                }
                digest.update(&block[..count]);
            }
            digest.finalize().into()
        })
        .collect()
}

fn command(binary: &Path, root: &Path) -> Command {
    let mut value = shield_core::utils::no_window_command(binary);
    value
        .current_dir(root)
        .arg("--app-home")
        .arg(root.join("home"))
        .env_remove("JAVA_TOOL_OPTIONS")
        .env_remove("_JAVA_OPTIONS")
        .env_remove("JDK_JAVA_OPTIONS")
        .env_remove("MOCIKA_SHIELD_KS_PASS")
        .env_remove("MOCIKA_SHIELD_KEY_PASS");
    value
}

fn run(command: &mut Command) -> Value {
    let output = command.output().unwrap();
    assert!(
        output.status.success(),
        "实际包内命令失败：{}",
        String::from_utf8_lossy(&output.stdout)
    );
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["type"], "result");
    value
}

fn verify(apk: &Path) {
    // 独立原工具在 ASCII 临时目录验签，不依赖被测应用的参数适配或工具查找。
    let reference = tempfile::tempdir().unwrap();
    assert!(reference.path().as_os_str().is_ascii());
    let copy = reference.path().join("verify.apk");
    fs::copy(apk, &copy).unwrap();
    let output = shield_core::utils::java_command(shield_core::utils::find_java().unwrap())
        .arg("-jar")
        .arg(std::env::var_os("SHIELD_TEST_APKSIGNER").expect("指定独立原签名工具"))
        .args(["verify", "--verbose", "--print-certs"])
        .arg(&copy)
        .output()
        .unwrap();
    assert!(output.status.success(), "独立原工具验签失败");
    if let Some(binding) = shield_core::inspect_signing_input(apk, || Ok(()))
        .unwrap()
        .certificate_binding
    {
        let expected = binding
            .iter()
            .map(|value| format!("{value:02X}"))
            .collect::<String>();
        assert!(String::from_utf8(output.stdout)
            .unwrap()
            .to_ascii_uppercase()
            .contains(&expected));
    }
    assert!(shield_core::utils::no_window_command(
        std::env::var_os("SHIELD_TEST_ZIPALIGN").expect("指定独立 SDK 对齐工具")
    )
    .args(["-c", "-P", "16", "4"])
    .arg(copy)
    .output()
    .unwrap()
    .status
    .success());
}

#[test]
#[ignore = "显式指定真实安装包内 CLI、自有 DEX APK 与 SDK；仅手动产物验收"]
fn 实际包内证书加固签名及_mcp不依赖源码资源覆盖() {
    let binary =
        PathBuf::from(std::env::var_os("SHIELD_PACKAGED_CLI").expect("指定实际安装包内 CLI"))
            .canonicalize()
            .unwrap();
    let before = snapshot(&binary);
    let fixture = PathBuf::from(std::env::var_os("SHIELD_TEST_APK").expect("指定自有测试 APK"));
    let source = fs::read(&fixture).unwrap();
    let directory = tempfile::Builder::new()
        .prefix("package-smoke-")
        .tempdir()
        .unwrap();
    let root = directory.path();
    for name in ["input", "output"] {
        fs::create_dir(root.join(name)).unwrap();
    }
    let input = root.join("input/中文 应用.apk");
    fs::write(&input, &source).unwrap();
    let created = run(command(&binary, root)
        .env("MOCIKA_SHIELD_KS_PASS", "package-smoke-not-user-secret")
        .args([
            "cert",
            "create",
            "--name",
            "实际包回归临时证书",
            "--file-name",
            "测试证书",
            "--alias",
            "中文Release",
            "--dname",
            "CN=包级回归",
            "--keystore-type",
            "pkcs12",
            "--validity-days",
            "7",
            "--set-default",
            "--v4",
            "false",
        ]));
    let id = created["data"]["certificate"]["id"].as_str().unwrap();
    run(command(&binary, root).args(["cert", "verify", id]));
    let signed = root.join("input/中文 已签名.apk");
    let output = command(&binary, root)
        .args(["sign", "--use-default-certificate", "--json", "--input"])
        .arg(&input)
        .arg("--output")
        .arg(&signed)
        .output()
        .unwrap();
    assert!(output.status.success(), "实际包独立签名失败");
    verify(&signed);
    for profile in ["standard", "android-api19"] {
        let protected = root.join(format!("output/{profile}.apk"));
        let output = command(&binary, root)
            .args([
                "protect",
                "--runtime-profile",
                profile,
                "--use-default-certificate",
                "--json",
                "--input",
            ])
            .arg(&signed)
            .arg("--output")
            .arg(&protected)
            .output()
            .unwrap();
        assert!(output.status.success(), "实际包加固签名失败：{profile}");
        assert!(shield_core::inspect_signing_input(&protected, || Ok(()))
            .unwrap()
            .certificate_binding
            .is_some());
        verify(&protected);
    }
    let mut server = command(&binary, root);
    server
        .args([
            "mcp",
            "serve",
            "--use-shared-settings",
            "--allow-execution",
            "--allow-certificate",
            id,
            "--input-root",
        ])
        .arg(root.join("input"))
        .arg("--output-root")
        .arg(root.join("output"));
    let mut session = client::Session::start(server);
    session.send(json!({"jsonrpc":"2.0", "id":2, "method":"tools/list"}));
    let listed = session.receive(2);
    assert_eq!(listed["result"]["tools"].as_array().unwrap().len(), 15);
    let result = session.call(
        3,
        "shield_sign",
        json!({"input":"中文 已签名.apk", "output":"mcp.apk"}),
    );
    assert_eq!(
        result["status"], "succeeded",
        "实际包 MCP 签名失败：{result}"
    );
    session.close();
    verify(&root.join("output/mcp.apk"));
    assert_eq!(fs::read(&input).unwrap(), source);
    assert_eq!(fs::read(&fixture).unwrap(), source);
    assert_eq!(snapshot(&binary), before, "测试不得修改共包命令或资源");
    println!("实际包内证书创建校验、普通签名、标准／工控加固签名、MCP 工具发现与共享签名、独立原工具及 SDK 验证通过");
}

//! 显式验收源码目录外的真实应用内 CLI；不启动 GUI，不修改应用包或用户签名材料。
#![cfg(all(feature = "agent-executor", target_os = "macos"))]
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

#[path = "support/execution_fixture.rs"]
mod real_fixture;

fn result(output: Output) -> Value {
    assert!(
        output.status.success(),
        "应用内命令未成功：{:?}",
        output.status
    );
    assert!(output.stderr.is_empty(), "不得回传原始错误日志");
    let frames: Vec<Value> = std::str::from_utf8(&output.stdout)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(frames.first().unwrap()["type"], "started");
    assert_eq!(frames.last().unwrap()["type"], "result");
    assert_eq!(
        frames
            .iter()
            .filter(|frame| frame["type"] == "result")
            .count(),
        1
    );
    frames.last().unwrap().clone()
}

fn command(fixture: &real_fixture::Fixture, binary: &Path, action: &str) -> Command {
    let mut command = fixture.command(binary);
    command
        .current_dir(fixture.root())
        .arg("--config")
        .arg(&fixture.config)
        .args(["agent", action, "-i"])
        .arg(&fixture.input)
        .arg("-o")
        .arg(&fixture.output);
    command
}

#[test]
#[ignore = "需显式提供源码目录外的真实 .app、授权小包与 SDK zipalign；不纳入日常 CI"]
fn 源码目录外应用内cli共享资源并独立完成规划加固签名() {
    let app =
        PathBuf::from(std::env::var_os("SHIELD_AGENT_TEST_APP").expect("请指定源码目录外的 .app"))
            .canonicalize()
            .unwrap();
    let repo = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap();
    assert!(
        !app.starts_with(&repo),
        "须实际移动应用包，不在源码树内验证"
    );
    let binary = app.join("Contents/MacOS/shield");
    let bundled = [
        "Contents/MacOS/shield",
        "Contents/MacOS/mocika-shield",
        "Contents/Resources/tools/apktool.jar",
        "Contents/Resources/tools/apksigner.jar",
        "Contents/Resources/resources/resources.zip",
        "Contents/Resources/resources/resources-api19.zip",
    ];
    let before: Vec<_> = bundled
        .iter()
        .map(|name| real_fixture::hash(&app.join(name)))
        .collect();
    assert!(!app.join("Contents/Resources/skills").exists());
    assert!(!app.join("Contents/Resources/integrations/skills").exists());
    let fixture = real_fixture::Fixture::new();
    // 技能从源码独立安装到本次临时项目，不由应用包提供，不改用户全局配置。
    let installed_skill = fixture
        .root()
        .join("Android 工程/.agents/skills/mocika-shield");
    for name in [
        "SKILL.md",
        "references/cli.md",
        "agents/openai.yaml",
        "scripts/resolve-cli.sh",
    ] {
        let target = installed_skill.join(name);
        fs::create_dir_all(target.parent().unwrap()).unwrap();
        fs::copy(repo.join("skills/mocika-shield").join(name), &target).unwrap();
    }
    let located = Command::new("/bin/bash")
        .arg(installed_skill.join("scripts/resolve-cli.sh"))
        .env("MOCIKA_SHIELD_CLI", &binary)
        .env_remove("BASH_ENV")
        .current_dir(fixture.root())
        .output()
        .unwrap();
    assert!(located.status.success());
    assert!(located.stderr.is_empty());
    let selected = PathBuf::from(
        std::str::from_utf8(&located.stdout)
            .unwrap()
            .trim_end_matches('\n'),
    );
    assert_eq!(selected, binary);
    let binary = selected;
    // 准备输入副本可以使用源码工具；实际规划/执行不指定任何工具或资源路径。
    let config = format!("schema_version=1\n[sign]\nkeystore={}\nkey_alias='agentfixture'\nkeystore_type='pkcs12'\nv1=true\nv2=true\nv3=true\nv4=false\n",
        serde_json::to_string(fixture.root().join("临时测试证书.p12").to_str().unwrap()).unwrap());
    fs::write(&fixture.config, config).unwrap();
    for operation in ["protect", "certificate"] {
        let doctor = fixture
            .command(&binary)
            .current_dir(fixture.root())
            .args(["doctor", "--operation", operation])
            .output()
            .unwrap();
        assert!(doctor.status.success());
        let doctor: Value = serde_json::from_slice(&doctor.stdout).unwrap();
        assert_eq!(doctor["status"], "ready");
        fixture.assert_no_tasks();
    }

    let compat = result(
        command(&fixture, &binary, "plan")
            .args(["--runtime-profile", "android-api19"])
            .output()
            .unwrap(),
    );
    assert!(matches!(
        compat["status"].as_str(),
        Some("ready" | "warning")
    ));
    fixture.assert_no_tasks();
    let planned = result(command(&fixture, &binary, "plan").output().unwrap());
    assert!(matches!(
        planned["status"].as_str(),
        Some("ready" | "warning")
    ));
    fixture.assert_no_tasks();
    let digest = planned["plan"]["sha256"].as_str().unwrap();
    let completed = result(
        command(&fixture, &binary, "protect")
            .args(["--expect-plan-sha256", digest])
            .output()
            .unwrap(),
    );
    assert_eq!(completed["status"], "succeeded");
    assert_eq!(completed["output_state"], "published");
    assert_eq!(completed["cleanup_state"], "complete");
    let actual: String = real_fixture::hash(&fixture.output)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    assert_eq!(completed["artifact"]["sha256"], actual);
    let signer = app.join("Contents/Resources/tools/apksigner.jar");
    assert_eq!(
        shield_core::extract_apk_cert_fingerprint(&fixture.input, Some(&fixture.signer)).unwrap(),
        shield_core::extract_apk_cert_fingerprint(&fixture.output, Some(&signer)).unwrap()
    );
    let zipalign = std::env::var_os("SHIELD_AGENT_TEST_ZIPALIGN").expect("请指定 SDK zipalign");
    assert!(Command::new(zipalign)
        .args(["-c", "-P", "16", "4"])
        .arg(&fixture.output)
        .status()
        .unwrap()
        .success());
    fixture.assert_inputs_unchanged();
    fixture.assert_no_tasks();
    let after: Vec<_> = bundled
        .iter()
        .map(|name| real_fixture::hash(&app.join(name)))
        .collect();
    assert_eq!(before, after, "共享应用包不得写入任务材料");
    assert!(Command::new("codesign")
        .args(["--verify", "--deep", "--strict"])
        .arg(&app)
        .status()
        .unwrap()
        .success());
}

#[test]
fn 应用内缺失资源不得借开发目录补齐() {
    let root = tempfile::tempdir().unwrap();
    let contents = root.path().join("中文 空格.app/Contents");
    let binary = contents.join("MacOS/shield");
    fs::create_dir_all(binary.parent().unwrap()).unwrap();
    fs::copy(env!("CARGO_BIN_EXE_shield"), &binary).unwrap();
    let doctor = Command::new(binary)
        .args(["doctor", "--operation", "protect"])
        .current_dir(root.path())
        .output()
        .unwrap();
    assert_eq!(doctor.status.code(), Some(1));
    let response: Value = serde_json::from_slice(&doctor.stdout).unwrap();
    let checks = response["data"]["checks"].as_array().unwrap();
    for name in ["apktool", "apksigner", "resources"] {
        let check = checks
            .iter()
            .find(|check| check["dependency"] == name)
            .unwrap();
        assert_eq!(check["code"], "FILE_UNAVAILABLE");
    }
}

//! 真实标准输入输出协议回归；夹具不代表用户 APK 或宿主 Agent 已验收。
#![cfg(all(feature = "mcp-server", any(target_os = "macos", target_os = "linux")))]
use serde_json::json;
use std::fs;
use std::os::unix::fs::{symlink, PermissionsExt};
use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};

#[path = "support/execution_fixture.rs"]
mod real_fixture;

#[path = "support/mcp_client.rs"]
mod client;
use client::Session;

fn fixture() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    for name in ["输入", "输出", "bin"] {
        fs::create_dir(dir.path().join(name)).unwrap();
    }
    fs::write(dir.path().join("输入/中文 样本.apk"), b"fixture-not-apk").unwrap();
    fs::write(dir.path().join("bin/java"), "#!/bin/sh\nif [ -n \"${MOCIKA_SHIELD_KS_PASS+x}${MOCIKA_SHIELD_KEY_PASS+x}\" ]; then printf x > \"$SHIELD_FIXTURE_ROOT/credential-leak\"; fi\nwhile :; do printf x >> \"$SHIELD_FIXTURE_ROOT/heartbeat\"; /bin/sleep 0.02; done\n").unwrap();
    fs::set_permissions(
        dir.path().join("bin/java"),
        fs::Permissions::from_mode(0o700),
    )
    .unwrap();
    dir
}
fn stopped(root: &Path) {
    assert!(
        !root.join("credential-leak").exists(),
        "无凭据需求的检查不得继承签名密码"
    );
    let before = fs::read(root.join("heartbeat")).unwrap();
    std::thread::sleep(Duration::from_millis(100));
    assert_eq!(
        before,
        fs::read(root.join("heartbeat")).unwrap(),
        "任务终态后后代仍在运行"
    );
}

#[test]
fn 真实协议发现仅三项只读工具且参数不能提升权限() {
    let dir = fixture();
    let mut session = Session::new(dir.path(), None, 10, false);
    session.send(json!({"jsonrpc":"2.0","id":2,"method":"tools/list"}));
    let tools = session.receive(2)["result"]["tools"]
        .as_array()
        .unwrap()
        .clone();
    assert_eq!(
        tools
            .iter()
            .map(|tool| tool["name"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["shield_doctor", "shield_preflight", "shield_plan"]
    );
    for tool in tools {
        assert_eq!(tool["annotations"]["readOnlyHint"], true);
    }
    assert_eq!(
        session.call(3, "shield_protect", json!({}))["code"],
        "TOOL_NOT_AVAILABLE"
    );
    for arguments in [
        json!({"password":"秘密"}),
        json!({"operation":"protect","config":"evil"}),
        json!({"operation":"arbitrary"}),
    ] {
        assert_eq!(
            session.call(4, "shield_doctor", arguments)["code"],
            "ARGUMENT_INVALID"
        );
    }
    assert_eq!(
        session.call(
            5,
            "shield_plan",
            json!({"input":"中文 样本.apk","output":"out.apk"})
        )["code"],
        "SIGNING_CONFIG_REQUIRED"
    );
    assert!(!dir.path().join("输出/out.apk").exists());
    session.close();
}

#[test]
fn 路径越界链接及输出占用不触发工具() {
    let dir = fixture();
    let outside = tempfile::tempdir().unwrap();
    symlink(outside.path(), dir.path().join("输入/链接")).unwrap();
    fs::write(dir.path().join("输出/存在.apk"), b"keep").unwrap();
    let mut session = Session::new(dir.path(), None, 10, true);
    for name in ["../outside.apk", "/tmp/a.apk", "链接/a.apk", "a\nb.apk"] {
        assert_eq!(
            session.call(2, "shield_preflight", json!({"input":name}))["code"],
            "PATH_NOT_ALLOWED"
        );
    }
    assert_eq!(
        session.call(
            3,
            "shield_plan",
            json!({"input":"中文 样本.apk","output":"存在.apk"})
        )["code"],
        "OUTPUT_EXISTS"
    );
    assert!(!dir.path().join("heartbeat").exists());
    assert_eq!(fs::read(dir.path().join("输出/存在.apk")).unwrap(), b"keep");
    session.close();
}

#[test]
fn 检查超时必须停止本次工具组且协议仍可使用() {
    let dir = fixture();
    let mut session = Session::new(dir.path(), None, 1, true);
    let result = session.call(2, "shield_doctor", json!({}));
    assert_eq!(result["code"], "TASK_TIMEOUT");
    stopped(dir.path());
    assert_eq!(
        session.call(3, "shield_protect", json!({}))["code"],
        "TOOL_NOT_AVAILABLE"
    );
    session.close();
}

#[test]
fn 取消和并发调用不丢失检查清场责任() {
    let dir = fixture();
    let mut session = Session::new(dir.path(), None, 15, true);
    session.send(json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"shield_doctor","arguments":{}}}));
    session.wait_file(&dir.path().join("heartbeat"), 2, Duration::from_secs(15));
    assert_eq!(
        session.call(3, "shield_doctor", json!({}))["code"],
        "SERVER_BUSY"
    );
    session
        .send(json!({"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":2}}));
    // 协议取消后不等待原请求响应；关闭会话必须先等待后台清场。
    session.close();
    stopped(dir.path());
}

#[test]
fn 输入断开会停止检查后代而非遗留后台任务() {
    let dir = fixture();
    let mut session = Session::new(dir.path(), None, 15, true);
    session.send(json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"shield_doctor","arguments":{}}}));
    session.wait_file(&dir.path().join("heartbeat"), 2, Duration::from_secs(15));
    session.close();
    stopped(dir.path());
}

#[test]
fn 固定配置改变后失败关闭且不输出原文() {
    let dir = fixture();
    let config = dir.path().join("shield-cli.toml");
    fs::write(&config, "schema_version=1\n").unwrap();
    let mut session = Session::new(dir.path(), Some(&config), 10, true);
    fs::write(&config, "schema_version=1\n# 不得回传的本地配置\n").unwrap();
    let result = session.call(2, "shield_doctor", json!({}));
    assert_eq!(result["code"], "TRUSTED_FILE_CHANGED");
    assert!(!result.to_string().contains("不得回传"));
    assert!(!dir.path().join("heartbeat").exists());
    session.close();
}

#[test]
fn 超限或畸形协议消息不能污染后续工具发现() {
    let dir = fixture();
    let mut session = Session::new(dir.path(), None, 10, true);
    session.send(
        json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{
        "name":"shield_doctor","arguments":{"payload":"x".repeat(70 * 1024)}}}),
    );
    session.raw(b"not-json\n");
    session.send(json!({"jsonrpc":"2.0","id":3,"method":"tools/list"}));
    assert_eq!(
        session.receive(3)["result"]["tools"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
    assert!(!dir.path().join("heartbeat").exists());
    session.close();
}

#[test]
fn 开启执行仍必须提供宿主固定签名配置() {
    let dir = fixture();
    let output = Command::new(env!("CARGO_BIN_EXE_shield"))
        .args(["mcp", "serve", "--allow-execution", "--input-root"])
        .arg(dir.path().join("输入"))
        .arg("--output-root")
        .arg(dir.path().join("输出"))
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("SIGNING_CONFIG_REQUIRED"));
}

#[test]
fn 显式允许才发现写工具且工具参数不能选择路径或凭据() {
    let dir = fixture();
    let config = dir.path().join("shield-cli.toml");
    fs::write(&config, "schema_version=1\n").unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_shield"));
    command
        .arg("--config")
        .arg(config)
        .args(["mcp", "serve", "--allow-execution", "--input-root"])
        .arg(dir.path().join("输入"))
        .arg("--output-root")
        .arg(dir.path().join("输出"));
    let mut session = Session::start(command);
    session.send(json!({"jsonrpc":"2.0","id":2,"method":"tools/list"}));
    let list = session.receive(2);
    let tools = list["result"]["tools"].as_array().unwrap();
    assert_eq!(tools.len(), 5);
    assert_eq!(tools[3]["name"], "shield_protect");
    assert_eq!(tools[3]["annotations"]["readOnlyHint"], false);
    assert_eq!(
        session.call(
            3,
            "shield_protect",
            json!({"plan_id":"a".repeat(32),"expect_plan_sha256":"b".repeat(64)})
        )["code"],
        "PLAN_NOT_AVAILABLE"
    );
    assert_eq!(session.call(4,"shield_protect",json!({"plan_id":"a".repeat(32),"expect_plan_sha256":"b".repeat(64),"output":"evil.apk"}))["code"],"ARGUMENT_INVALID");
    assert_eq!(
        session.call(
            5,
            "shield_execution_status",
            json!({"plan_id":"a".repeat(32)})
        )["code"],
        "PLAN_NOT_AVAILABLE"
    );
    session.close();
}

#[test]
fn 宿主缺少凭据时计划失败而不自动读取其他凭据来源() {
    let dir = fixture();
    let config = planning_fixture(dir.path());
    let mut command = Command::new(env!("CARGO_BIN_EXE_shield"));
    command
        .arg("--config")
        .arg(config)
        .args(["mcp", "serve", "--allow-execution", "--input-root"])
        .arg(dir.path().join("输入"))
        .arg("--output-root")
        .arg(dir.path().join("输出"))
        .args(["--timeout-seconds", "5"])
        .env("PATH", dir.path().join("bin"))
        .env("SHIELD_FIXTURE_ROOT", dir.path())
        .env_remove("MOCIKA_SHIELD_KS_PASS")
        .env_remove("MOCIKA_SHIELD_KEY_PASS")
        .env_remove("JAVA_HOME");
    let mut session = Session::start(command);
    let result = session.call(
        2,
        "shield_plan",
        json!({"input":"中文 样本.apk","output":"out.apk"}),
    );
    assert_eq!(result["code"], "CREDENTIAL_UNAVAILABLE");
    assert!(result["plan_id"].is_null());
    assert!(!dir.path().join("heartbeat").exists());
    assert!(!dir.path().join("输出/out.apk").exists());
    session.close();
}

fn planning_fixture(root: &Path) -> std::path::PathBuf {
    fs::write(
        root.join("bin/java"),
        "#!/bin/sh\necho 'openjdk version \"17.0.1\"' >&2\n",
    )
    .unwrap();
    fs::write(root.join("bin/keytool"), "#!/bin/sh\nwhile :; do printf x >> \"$SHIELD_FIXTURE_ROOT/heartbeat\"; /bin/sleep 0.02; done\n").unwrap();
    fs::set_permissions(root.join("bin/keytool"), fs::Permissions::from_mode(0o700)).unwrap();
    for name in ["fixture.p12", "tool.jar", "resources.zip"] {
        fs::write(root.join(name), b"fixture").unwrap();
    }
    let config = root.join("shield-cli.toml");
    fs::write(&config, "schema_version=1\n[protect]\napktool='tool.jar'\napksigner='tool.jar'\nresources='resources.zip'\n[sign]\nkeystore='fixture.p12'\nkey_alias='fixture'\n").unwrap();
    config
}

#[test]
fn 受控计划超时保留超时原因并停止其证书工具() {
    let dir = fixture();
    let config = planning_fixture(dir.path());
    let mut session = Session::new(dir.path(), Some(&config), 10, true);
    session.send(
        json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"shield_plan",
        "arguments":{"input":"中文 样本.apk","output":"out.apk"},"_meta":{"progressToken":"准备门槛"}}}),
    );
    session.wait_stage("prepare");
    session.wait_file(&dir.path().join("heartbeat"), 3, Duration::from_secs(10));
    let result = session.receive(3)["result"]["structuredContent"].clone();
    assert_eq!(result["status"], "timed_out", "超时回执不符：{result}");
    assert_eq!(result["code"], "TASK_TIMEOUT");
    let before = fs::read(dir.path().join("heartbeat")).unwrap();
    std::thread::sleep(Duration::from_millis(100));
    assert_eq!(before, fs::read(dir.path().join("heartbeat")).unwrap());
    assert!(!dir.path().join("输出/out.apk").exists());
    session.close();
}

#[test]
fn 受控计划断开后先收尾且不生成最终文件() {
    let dir = fixture();
    let config = planning_fixture(dir.path());
    let mut session = Session::new(dir.path(), Some(&config), 15, true);
    session.send(
        json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"shield_plan",
        "arguments":{"input":"中文 样本.apk","output":"out.apk"},"_meta":{"progressToken":"准备门槛"}}}),
    );
    // 先同步实际 CLI 准备阶段，不把调试执行器身份复核算成证书工具已运行。
    let started = Instant::now();
    session.wait_stage("prepare");
    let prepared = started.elapsed();
    session.wait_file(&dir.path().join("heartbeat"), 3, Duration::from_secs(15));
    eprintln!(
        "准备通知等待：{prepared:?}；证书工具等待：{:?}",
        started.elapsed() - prepared
    );
    session.close();
    let before = fs::read(dir.path().join("heartbeat")).unwrap();
    std::thread::sleep(Duration::from_millis(100));
    assert_eq!(before, fs::read(dir.path().join("heartbeat")).unwrap());
    assert!(!dir.path().join("输出/out.apk").exists());
}

#[test]
#[ignore = "需显式指定授权 APK，并具备真实 Java、签名工具和两套资源"]
fn 当前机器真实协议完成环境预检及受控计划但不执行加固() {
    let fixture = real_fixture::Fixture::new();
    let input = fixture.root().join("输入");
    let output = fixture.root().join("输出");
    fs::create_dir(&input).unwrap();
    fs::create_dir(&output).unwrap();
    let sample = input.join("已签名 副本.apk");
    fs::copy(&fixture.input, &sample).unwrap();
    let before = real_fixture::hash(&sample);
    let mut command = fixture.command(Path::new(env!("CARGO_BIN_EXE_shield")));
    command
        .arg("--config")
        .arg(&fixture.config)
        .args(["mcp", "serve", "--input-root"])
        .arg(&input)
        .arg("--output-root")
        .arg(&output)
        .args(["--timeout-seconds", "60"]);
    let mut session = Session::start(command);
    assert_eq!(
        session.call(2, "shield_doctor", json!({"operation":"certificate"}))["status"],
        "ready"
    );
    let preflight = session.call(3, "shield_preflight", json!({"input":"已签名 副本.apk"}));
    assert!(
        preflight["status"] == "ready" || preflight["status"] == "warning",
        "真实预检未就绪：{preflight}"
    );
    let plan = session.call(
        4,
        "shield_plan",
        json!({"input":"已签名 副本.apk","output":"计划 产物.apk"}),
    );
    assert!(
        plan["status"] == "ready" || plan["status"] == "warning",
        "真实计划未就绪：{plan}"
    );
    assert_eq!(plan["plan"]["sha256"].as_str().unwrap().len(), 64);
    assert_eq!(plan["plan"]["private_key_access_checked"], false);
    assert_eq!(plan["cleanup_state"], "complete");
    assert!(!plan.to_string().contains(fixture.root().to_str().unwrap()));
    assert!(!output.join("计划 产物.apk").exists());
    session.close();
    assert_eq!(real_fixture::hash(&sample), before);
    assert!(!fixture.output.exists());
    assert!(fixture.signer.is_file());
    fixture.assert_inputs_unchanged();
    for entry in fs::read_dir(&output).unwrap() {
        assert!(!entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".mocika-"));
    }
    fixture.assert_no_tasks();
}

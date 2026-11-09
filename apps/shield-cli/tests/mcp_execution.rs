//! 当前 Mac 的显式真实工具验收；不纳入默认重型测试，不修改生产证书或设备。
#![cfg(all(feature = "mcp-server", target_os = "macos"))]
use serde_json::{json, Value};
use std::{
    fs,
    path::Path,
    time::{Duration, Instant},
};
#[path = "support/mcp_client.rs"]
mod client;
#[path = "support/execution_fixture.rs"]
mod real_fixture;
use client::Session;

struct Fixture {
    real: real_fixture::Fixture,
    config_hash: [u8; 32],
}
impl Fixture {
    fn new() -> Self {
        let real = real_fixture::Fixture::new();
        for name in ["输入", "输出"] {
            fs::create_dir(real.root().join(name)).unwrap();
        }
        fs::copy(&real.input, real.root().join("输入/中文 样本.apk")).unwrap();
        let config_hash = real_fixture::hash(&real.config);
        Self { real, config_hash }
    }
    fn start(&self, wrong_key: bool) -> Session {
        let mut command = self.real.command(Path::new(env!("CARGO_BIN_EXE_shield")));
        command
            .arg("--config")
            .arg(&self.real.config)
            .args(["mcp", "serve", "--allow-execution", "--input-root"])
            .arg(self.real.root().join("输入"))
            .arg("--output-root")
            .arg(self.real.root().join("输出"))
            .args(["--timeout-seconds", "60"]);
        if wrong_key {
            command.env("MOCIKA_SHIELD_KEY_PASS", "fixture-wrong-key-not-user-data");
        }
        Session::start(command)
    }
    fn plan(&self, session: &mut Session, output: &str) -> Value {
        let result = session.call(
            2,
            "shield_plan",
            json!({"input":"中文 样本.apk","output":output}),
        );
        assert!(
            matches!(result["status"].as_str(), Some("ready" | "warning")),
            "真实计划未就绪：{result}"
        );
        assert_eq!(result["plan_id"].as_str().unwrap().len(), 32);
        result
    }
    fn unchanged(&self) {
        assert!(!self.real.output.exists(), "不得使用未授权的默认输出位置");
        assert_eq!(real_fixture::hash(&self.real.config), self.config_hash);
        assert_eq!(
            real_fixture::hash(&self.real.root().join("输入/中文 样本.apk")),
            real_fixture::hash(&self.real.input)
        );
        self.real.assert_inputs_unchanged();
        for name in ["输入", "输出"] {
            for entry in fs::read_dir(self.real.root().join(name)).unwrap() {
                assert!(
                    !entry
                        .unwrap()
                        .file_name()
                        .to_string_lossy()
                        .starts_with(".mocika-"),
                    "实际授权目录不应残留任务材料"
                );
            }
        }
        self.real.assert_no_tasks();
    }
}
fn reference(plan: &Value) -> Value {
    json!({"plan_id":plan["plan_id"],"expect_plan_sha256":plan["plan"]["sha256"]})
}
fn execute(session: &mut Session, plan: &Value) {
    session.send(
        json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{
        "name":"shield_protect","arguments":reference(plan),"_meta":{"progressToken":"执行进度"}}}),
    );
}
fn status(session: &mut Session, plan: &Value) -> Value {
    session.call(
        5,
        "shield_execution_status",
        json!({"plan_id":plan["plan_id"]}),
    )
}
fn finished(session: &mut Session, plan: &Value) -> Value {
    let until = Instant::now() + Duration::from_secs(25);
    loop {
        let result = status(session, plan);
        if result["state"] == "finished" {
            return result;
        }
        assert!(Instant::now() < until, "取消后未取得有限终态");
        std::thread::sleep(Duration::from_millis(30));
    }
}

#[test]
#[ignore = "需要授权 APK、真实 Java/资源和 SDK zipalign；显式验收，不加入日常重型 CI"]
fn 真实协议执行验签查询且重复执行和晚取消不破坏产物() {
    let fixture = Fixture::new();
    let mut session = fixture.start(false);
    let plan = fixture.plan(&mut session, "已加固 签名.apk");
    let mut bad = reference(&plan);
    bad["expect_plan_sha256"] = json!("0".repeat(64));
    assert_eq!(
        session.call(4, "shield_protect", bad)["code"],
        "PLAN_DIGEST_MISMATCH"
    );
    assert_eq!(status(&mut session, &plan)["state"], "planned");
    execute(&mut session, &plan);
    session.wait_stage("unpack");
    let running = status(&mut session, &plan);
    assert_eq!(running["state"], "running");
    assert_eq!(
        session.call(6, "shield_protect", reference(&plan))["code"],
        "SERVER_BUSY"
    );
    let result = session.receive(3)["result"]["structuredContent"].clone();
    assert_eq!(result["status"], "succeeded", "真实执行未成功：{result}");
    assert_eq!(result["output_state"], "published");
    assert_eq!(result["cleanup_state"], "complete");
    let output = fixture.real.root().join("输出/已加固 签名.apk");
    let hash = real_fixture::hash(&output);
    let digest: String = hash.iter().map(|byte| format!("{byte:02x}")).collect();
    assert_eq!(result["artifact"]["sha256"], digest);
    let java = shield_core::utils::find_java().unwrap();
    assert!(shield_core::utils::java_command(java)
        .arg("-jar")
        .arg(&fixture.real.signer)
        .arg("verify")
        .arg(&output)
        .output()
        .unwrap()
        .status
        .success());
    assert_eq!(
        shield_core::extract_apk_cert_fingerprint(&fixture.real.input, Some(&fixture.real.signer))
            .unwrap(),
        shield_core::extract_apk_cert_fingerprint(&output, Some(&fixture.real.signer)).unwrap()
    );
    let align = std::env::var_os("SHIELD_AGENT_TEST_ZIPALIGN").expect("请显式指定 SDK zipalign");
    assert!(shield_core::utils::no_window_command(align)
        .args(["-c", "-P", "16", "4"])
        .arg(&output)
        .output()
        .unwrap()
        .status
        .success());
    let mut before = 0.0;
    for notification in &session.notifications {
        let progress = notification["params"]["progress"].as_f64().unwrap();
        assert!(progress > before);
        before = progress;
    }
    assert_eq!(status(&mut session, &plan)["status"], "succeeded");
    assert_eq!(
        session.call(7, "shield_protect", reference(&plan))["code"],
        "PLAN_ALREADY_CONSUMED"
    );
    session
        .send(json!({"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":3}}));
    assert_eq!(status(&mut session, &plan)["status"], "succeeded");
    assert_eq!(real_fixture::hash(&output), hash);
    session.close();
    fixture.unchanged();
}

#[test]
#[ignore = "需要授权 APK、真实 Java 和资源；显式验收执行取消、状态查询及断开"]
fn 真实执行取消后可查询终态且断开不留下最终产物() {
    for disconnect in [false, true] {
        let fixture = Fixture::new();
        let mut session = fixture.start(false);
        let plan = fixture.plan(&mut session, "取消 产物.apk");
        execute(&mut session, &plan);
        session.wait_stage("unpack");
        if disconnect {
            session.close();
        } else {
            session.send(json!({"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":3}}));
            let result = finished(&mut session, &plan);
            assert_eq!(
                result["status"], "cancelled",
                "取消结果不符合预期：{result}"
            );
            assert_eq!(result["output_state"], "not_published");
            assert_eq!(result["cleanup_state"], "complete");
            assert_eq!(
                session.call(7, "shield_protect", reference(&plan))["code"],
                "PLAN_ALREADY_CONSUMED"
            );
            session.close();
        }
        assert!(!fixture.real.root().join("输出/取消 产物.apk").exists());
        fixture.unchanged();
    }
}

#[test]
#[ignore = "需要授权 APK、真实 Java 和资源；显式验收签名失败、输出冲突和配置漂移"]
fn 真实失败与执行前阻断也消费计划并保留有限诊断() {
    let fixture = Fixture::new();
    let mut session = fixture.start(true);
    let plan = fixture.plan(&mut session, "失败 产物.apk");
    let result = session.call(3, "shield_protect", reference(&plan));
    assert_eq!(
        result["status"], "failed",
        "错误私钥密码未得到明确失败：{result}"
    );
    assert_eq!(result["output_state"], "not_published");
    assert!(!result.to_string().contains("fixture-wrong-key"));
    assert!(!fixture.real.root().join("输出/失败 产物.apk").exists());
    assert_eq!(status(&mut session, &plan)["status"], "failed");
    assert_eq!(
        session.call(7, "shield_protect", reference(&plan))["code"],
        "PLAN_ALREADY_CONSUMED"
    );
    let plan = fixture.plan(&mut session, "占用.apk");
    fs::write(fixture.real.root().join("输出/占用.apk"), b"keep").unwrap();
    let blocked = session.call(3, "shield_protect", reference(&plan));
    assert_eq!(blocked["code"], "OUTPUT_EXISTS");
    assert_eq!(blocked["cleanup_state"], "not_needed");
    assert_eq!(blocked["session_execution_blocked"], false);
    assert_eq!(
        fs::read(fixture.real.root().join("输出/占用.apk")).unwrap(),
        b"keep"
    );
    assert_eq!(
        session.call(7, "shield_protect", reference(&plan))["code"],
        "PLAN_ALREADY_CONSUMED"
    );
    let plan = fixture.plan(&mut session, "漂移.apk");
    let config = fs::read(&fixture.real.config).unwrap();
    let mut changed = config.clone();
    changed.extend_from_slice(b"\n# fixture\n");
    fs::write(&fixture.real.config, changed).unwrap();
    assert_eq!(
        session.call(3, "shield_protect", reference(&plan))["code"],
        "TRUSTED_FILE_CHANGED"
    );
    assert_eq!(status(&mut session, &plan)["state"], "finished");
    assert!(!fixture.real.root().join("输出/漂移.apk").exists());
    session.close();
    fs::write(&fixture.real.config, config).unwrap();
    fixture.unchanged();
}

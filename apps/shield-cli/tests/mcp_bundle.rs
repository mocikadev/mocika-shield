//! 完整应用包的 MCP 专项；只使用外置临时证书，不修改宿主配置或启动 GUI。
#![cfg(all(feature = "mcp-server", target_os = "macos"))]
use serde_json::{json, Value};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    time::{Duration, Instant},
};
#[path = "support/mcp_client.rs"]
mod client;
#[path = "support/execution_fixture.rs"]
mod real_fixture;
use client::Session;

struct Bundle {
    app: PathBuf,
    real: real_fixture::Fixture,
    config_hash: [u8; 32],
    before: Vec<[u8; 32]>,
}
const FILES: [&str; 7] = [
    "Contents/MacOS/shield",
    "Contents/MacOS/mocika-shield",
    "Contents/Resources/tools/apktool.jar",
    "Contents/Resources/tools/apksigner.jar",
    "Contents/Resources/resources/resources.zip",
    "Contents/Resources/resources/resources-api19.zip",
    "Contents/_CodeSignature/CodeResources",
];
impl Bundle {
    fn new() -> Self {
        let app =
            PathBuf::from(std::env::var_os("SHIELD_AGENT_TEST_APP").expect("请指定完整 .app"))
                .canonicalize()
                .unwrap();
        let repo = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()
            .unwrap();
        assert!(!app.starts_with(repo), "必须把完整应用包复制到源码目录外");
        let before = FILES
            .iter()
            .map(|name| real_fixture::hash(&app.join(name)))
            .collect();
        assert!(!app.join("Contents/Resources/skills").exists());
        assert!(!app.join("Contents/Resources/integrations/skills").exists());
        let real = real_fixture::Fixture::new();
        for name in ["输入", "输出"] {
            fs::create_dir(real.root().join(name)).unwrap();
        }
        fs::copy(&real.input, real.root().join("输入/中文 样本.apk")).unwrap();
        // 夹具准备可以用源码工具；真正 MCP 任务不覆盖工具或双资源路径。
        let config = format!("schema_version=1\n[sign]\nkeystore={}\nkey_alias='agentfixture'\nkeystore_type='pkcs12'\nv1=true\nv2=true\nv3=true\nv4=false\n",
            serde_json::to_string(real.root().join("临时测试证书.p12").to_str().unwrap()).unwrap());
        fs::write(&real.config, config).unwrap();
        let config_hash = real_fixture::hash(&real.config);
        Self {
            app,
            real,
            config_hash,
            before,
        }
    }
    fn start(&self, execution: bool) -> Session {
        let mut command = self.real.command(&self.app.join("Contents/MacOS/shield"));
        command
            .current_dir(self.real.root())
            .arg("--config")
            .arg(&self.real.config)
            .args(["mcp", "serve", "--input-root"])
            .arg(self.real.root().join("输入"))
            .arg("--output-root")
            .arg(self.real.root().join("输出"))
            .args(["--timeout-seconds", "60"]);
        if execution {
            command.arg("--allow-execution");
        }
        Session::start(command)
    }
    fn unchanged(&self) {
        self.real.assert_inputs_unchanged();
        assert_eq!(real_fixture::hash(&self.real.config), self.config_hash);
        assert_eq!(
            real_fixture::hash(&self.real.root().join("输入/中文 样本.apk")),
            real_fixture::hash(&self.real.input)
        );
        assert!(!self.real.output.exists());
        for name in ["输入", "输出"] {
            for entry in fs::read_dir(self.real.root().join(name)).unwrap() {
                assert!(
                    !entry
                        .unwrap()
                        .file_name()
                        .to_string_lossy()
                        .starts_with(".mocika-"),
                    "授权目录不得残留任务材料"
                );
            }
        }
        assert_eq!(
            self.before,
            FILES
                .iter()
                .map(|name| real_fixture::hash(&self.app.join(name)))
                .collect::<Vec<_>>()
        );
        assert!(Command::new("codesign")
            .args(["--verify", "--deep", "--strict"])
            .arg(&self.app)
            .status()
            .unwrap()
            .success());
        self.real.assert_no_tasks();
    }
}
fn list(session: &mut Session, count: usize) {
    session.send(json!({"jsonrpc":"2.0","id":10,"method":"tools/list"}));
    assert_eq!(
        session.receive(10)["result"]["tools"]
            .as_array()
            .unwrap()
            .len(),
        count
    );
}
fn plan(session: &mut Session, output: &str, profile: &str) -> Value {
    let plan = session.call(
        2,
        "shield_plan",
        json!({"input":"中文 样本.apk","output":output,"runtime_profile":profile}),
    );
    assert!(
        matches!(plan["status"].as_str(), Some("ready" | "warning")),
        "应用内计划失败：{plan}"
    );
    plan
}
fn reference(plan: &Value) -> Value {
    json!({"plan_id":plan["plan_id"],"expect_plan_sha256":plan["plan"]["sha256"]})
}
fn execute(session: &mut Session, plan: &Value) {
    session.send(
        json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{
        "name":"shield_protect","arguments":reference(plan),"_meta":{"progressToken":"共包验收"}}}),
    );
}
fn status(session: &mut Session, plan: &Value) -> Value {
    session.call(
        5,
        "shield_execution_status",
        json!({"plan_id":plan["plan_id"]}),
    )
}

#[test]
#[ignore = "须显式提供源码外完整 .app、自有 APK 和 SDK zipalign；不加入日常重型 CI"]
fn 完整应用内mcp共享资源完成只读执行验签及取消断开() {
    let bundle = Bundle::new();
    let mut readonly = bundle.start(false);
    list(&mut readonly, 3);
    assert_eq!(
        readonly.call(11, "shield_doctor", json!({}))["status"],
        "ready"
    );
    let preflight = readonly.call(12, "shield_preflight", json!({"input":"中文 样本.apk"}));
    assert!(
        matches!(preflight["status"].as_str(), Some("ready" | "warning")),
        "预检失败：{preflight}"
    );
    // 兼容资源也必须来自完整应用包，不借源码路径补齐。
    let compat = plan(&mut readonly, "工控 计划.apk", "android-api19");
    assert_eq!(
        readonly.call(3, "shield_protect", reference(&compat))["code"],
        "TOOL_NOT_AVAILABLE"
    );
    readonly.close();
    assert!(!bundle.real.root().join("输出/工控 计划.apk").exists());
    bundle.unchanged();

    let mut session = bundle.start(true);
    list(&mut session, 5);
    let planned = plan(&mut session, "已加固 签名.apk", "standard");
    execute(&mut session, &planned);
    session.wait_stage("unpack");
    assert_eq!(status(&mut session, &planned)["state"], "running");
    let result = session.receive(3)["result"]["structuredContent"].clone();
    assert_eq!(result["status"], "succeeded", "共包执行失败：{result}");
    assert_eq!(result["output_state"], "published");
    assert_eq!(result["cleanup_state"], "complete");
    let output = bundle.real.root().join("输出/已加固 签名.apk");
    let hash = real_fixture::hash(&output);
    assert_eq!(
        result["artifact"]["sha256"],
        hash.iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    );
    let signer = bundle.app.join("Contents/Resources/tools/apksigner.jar");
    assert!(
        shield_core::utils::java_command(shield_core::utils::find_java().unwrap())
            .arg("-jar")
            .arg(&signer)
            .arg("verify")
            .arg(&output)
            .output()
            .unwrap()
            .status
            .success()
    );
    assert_eq!(
        shield_core::extract_apk_cert_fingerprint(&bundle.real.input, Some(&bundle.real.signer))
            .unwrap(),
        shield_core::extract_apk_cert_fingerprint(&output, Some(&signer)).unwrap()
    );
    let align = std::env::var_os("SHIELD_AGENT_TEST_ZIPALIGN").expect("请提供 SDK zipalign");
    assert!(Command::new(align)
        .args(["-c", "-P", "16", "4"])
        .arg(&output)
        .status()
        .unwrap()
        .success());
    assert_eq!(status(&mut session, &planned)["status"], "succeeded");
    assert_eq!(
        session.call(7, "shield_protect", reference(&planned))["code"],
        "PLAN_ALREADY_CONSUMED"
    );
    session.close();
    bundle.unchanged();

    for disconnect in [false, true] {
        let mut session = bundle.start(true);
        let planned = plan(&mut session, "取消 产物.apk", "standard");
        execute(&mut session, &planned);
        session.wait_stage("unpack");
        if !disconnect {
            session.send(json!({"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":3}}));
            let until = Instant::now() + Duration::from_secs(25);
            let result = loop {
                let result = status(&mut session, &planned);
                if result["state"] == "finished" {
                    break result;
                }
                assert!(Instant::now() < until, "取消后没有可查询终态");
                std::thread::sleep(Duration::from_millis(30));
            };
            assert_eq!(result["status"], "cancelled");
            assert_eq!(result["output_state"], "not_published");
            assert_eq!(result["cleanup_state"], "complete");
        }
        session.close();
        assert!(!bundle.real.root().join("输出/取消 产物.apk").exists());
        assert_eq!(real_fixture::hash(&output), hash, "取消不能影响已交付产物");
        bundle.unchanged();
    }
}

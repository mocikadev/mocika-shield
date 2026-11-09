//! MCP 标准流测试客户端；只用于显式启动的测试程序，不修改宿主配置。
#![allow(dead_code)]
use serde_json::{json, Value};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};
// 调试 CLI 的文件身份摘要较重，多个夹具同时启动会竞争准备阶段的固定时间窗口。
// 只串行化测试进程启动/存活区间；同一会话内的并发请求、取消和查询仍实际并发。
static PROCESS_FIXTURES: Mutex<()> = Mutex::new(());
pub struct Session {
    child: Child,
    input: Option<ChildStdin>,
    output: Receiver<Value>,
    pub notifications: Vec<Value>,
    pending: HashMap<u64, Value>,
    serial: Option<MutexGuard<'static, ()>>,
}
impl Session {
    pub fn new(root: &Path, config: Option<&Path>, timeout: u64, hanging_java: bool) -> Self {
        let mut command = Command::new(env!("CARGO_BIN_EXE_shield"));
        if let Some(config) = config {
            command.arg("--config").arg(config);
        }
        command
            .args(["mcp", "serve", "--input-root"])
            .arg(root.join("输入"))
            .arg("--output-root")
            .arg(root.join("输出"))
            .args(["--timeout-seconds", &timeout.to_string()])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .env_remove("JAVA_HOME")
            .env_remove("JAVA_TOOL_OPTIONS")
            .env_remove("_JAVA_OPTIONS")
            .env_remove("JDK_JAVA_OPTIONS")
            .env("MOCIKA_SHIELD_KS_PASS", "fixture-secret-not-user-data")
            .env("SHIELD_FIXTURE_ROOT", root);
        if hanging_java {
            command.env("PATH", root.join("bin"));
        }
        Self::start(command)
    }
    pub fn start(mut command: Command) -> Self {
        let serial = PROCESS_FIXTURES
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        let mut child = command.spawn().unwrap();
        let input = child.stdin.take();
        let stdout = child.stdout.take().unwrap();
        let (send, output) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else {
                    break;
                };
                let Ok(value) = serde_json::from_str(&line) else {
                    break;
                };
                if send.send(value).is_err() {
                    break;
                }
            }
        });
        let mut session = Self {
            child,
            input,
            output,
            notifications: Vec::new(),
            pending: HashMap::new(),
            serial: Some(serial),
        };
        session.send(json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{
            "protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"本地协议回归","version":"1"}}}));
        let response = session.receive(1);
        assert_eq!(response["result"]["protocolVersion"], "2025-11-25");
        assert!(response["result"]["capabilities"]["tools"].is_object());
        session.send(json!({"jsonrpc":"2.0","method":"notifications/initialized"}));
        session
    }
    pub fn send(&mut self, value: Value) {
        writeln!(self.input.as_mut().unwrap(), "{value}").unwrap();
    }
    pub fn receive(&mut self, id: u64) -> Value {
        if let Some(value) = self.pending.remove(&id) {
            return value;
        }
        let deadline = Instant::now() + Duration::from_secs(90);
        loop {
            let value = self
                .output
                .recv_timeout(deadline.saturating_duration_since(Instant::now()))
                .expect("未收到合法 MCP 响应");
            if value["method"] == "notifications/progress" {
                self.notifications.push(value.clone());
            }
            if value["id"] == id {
                return value;
            }
            if let Some(other) = value["id"].as_u64() {
                self.pending.insert(other, value);
            }
        }
    }
    pub fn call(&mut self, id: u64, name: &str, arguments: Value) -> Value {
        self.send(json!({"jsonrpc":"2.0","id":id,"method":"tools/call","params":{"name":name,"arguments":arguments}}));
        self.receive(id)["result"]["structuredContent"].clone()
    }

    pub fn raw(&mut self, bytes: &[u8]) {
        self.input.as_mut().unwrap().write_all(bytes).unwrap();
    }
    pub fn wait_stage(&mut self, stage: &str) {
        let until = Instant::now() + Duration::from_secs(60);
        loop {
            let value = self
                .output
                .recv_timeout(until.saturating_duration_since(Instant::now()))
                .expect("执行未到达目标阶段");
            if value["method"] == "notifications/progress" {
                let matched = value["params"]["message"] == stage;
                self.notifications.push(value);
                if matched {
                    return;
                }
            } else if value["id"] == 3 {
                panic!("目标阶段前已收到执行终态：{value}");
            } else if let Some(id) = value["id"].as_u64() {
                self.pending.insert(id, value);
            }
        }
    }

    pub fn close(&mut self) {
        drop(self.input.take());
        let deadline = Instant::now() + Duration::from_secs(25);
        while self.child.try_wait().unwrap().is_none() {
            if Instant::now() > deadline {
                let _ = self.child.kill();
                let _ = self.child.wait();
                panic!("MCP 未按期限退出，不能视为清理通过");
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        drop(self.serial.take());
    }
}
impl Drop for Session {
    fn drop(&mut self) {
        if self.child.try_wait().ok().flatten().is_none() {
            self.close();
        }
    }
}

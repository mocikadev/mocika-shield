//! CLI 到 MCP 的结果白名单；不转发自由文本、路径或原始 stderr。
use super::policy::Result;
use serde_json::{json, Value};
mod management;
pub(super) use management::{decode_management, Management};

fn events(bytes: &[u8], operation: &str) -> Result<Vec<Value>> {
    if bytes.is_empty() || !bytes.ends_with(b"\n") {
        return Err("CLI_PROTOCOL_INVALID");
    }
    let mut events = Vec::new();
    for line in bytes[..bytes.len() - 1].split(|b| *b == b'\n') {
        if events.len() >= 32 || line.len() > 64 * 1024 {
            return Err("CLI_PROTOCOL_INVALID");
        }
        let value: Value = serde_json::from_slice(line).map_err(|_| "CLI_PROTOCOL_INVALID")?;
        if value["schema_version"] != 1
            || value["operation"] != operation
            || value["tool_version"] != env!("CARGO_PKG_VERSION")
        {
            return Err("CLI_PROTOCOL_INVALID");
        }
        events.push(value);
    }
    Ok(events)
}
pub(super) fn decode(bytes: &[u8], operation: &str, exit: i32) -> Result<Value> {
    let events = events(bytes, operation)?;
    if matches!(operation, "agent_plan" | "agent_protect" | "agent_sign") {
        return controlled(&events, operation, exit);
    }
    if events.len() != 1 {
        return Err("CLI_PROTOCOL_INVALID");
    }
    let event = &events[0];
    if event["type"] == "error" && exit == 1 {
        return Ok(
            json!({"schema_version":1,"operation":operation,"status":"blocked","code":code(&event["code"])?}),
        );
    }
    let status = status(&event["status"])?;
    if event["type"] != "result" || exit != i32::from(status == "blocked") {
        return Err("CLI_PROTOCOL_INVALID");
    }
    let data = &event["data"];
    if operation == "cert" {
        let rows = data["certificates"]
            .as_array()
            .ok_or("CLI_PROTOCOL_INVALID")?;
        if rows.len() > 1024 {
            return Err("CLI_PROTOCOL_INVALID");
        }
        let rows=rows.iter().map(|row| {
            let id=row["id"].as_str().ok_or("CLI_PROTOCOL_INVALID")?;
            if id.is_empty() || id.len()>128 || !id.bytes().all(|b|b.is_ascii_alphanumeric() || matches!(b,b'-'|b'_'|b'.')) {return Err("CLI_PROTOCOL_INVALID");}
            let mut versions=Vec::new();
            for key in ["sign_v1","sign_v2","sign_v3","sign_v4"] {
                versions.push(row[key].as_bool().ok_or("CLI_PROTOCOL_INVALID")?);
            }
            Ok(json!({"id":id,"is_default":row["is_default"].as_bool().ok_or("CLI_PROTOCOL_INVALID")?,"signing_versions":versions}))
        }).collect::<Result<Vec<_>>>()?;
        return Ok(
            json!({"schema_version":1,"operation":"cert","status":status,"data":{"certificates":rows}}),
        );
    }
    let checks = checks(&data["checks"], operation == "doctor")?;
    let safe = match operation {
        "doctor" if data["scope"] == "dependency_presence" => json!({
            "scope":"dependency_presence", "checks":checks,
            "java_major": if data["java_major"].is_null() { Value::Null } else { json!(number(&data["java_major"])?) },
        }),
        "preflight" if data["scope"] == "apk_preflight" => {
            let facts = &data["facts"];
            let abis = facts["native_abis"]
                .as_array()
                .ok_or("CLI_PROTOCOL_INVALID")?;
            if abis.len() > 64 {
                return Err("CLI_PROTOCOL_INVALID");
            }
            let mut safe_facts = json!({"native_abi_count":abis.len()});
            for key in [
                "apk_size",
                "dex_count",
                "dex_total_size",
                "native_library_count",
                "compressed_native_library_count",
            ] {
                safe_facts[key] = json!(number(&facts[key])?);
            }
            json!({"scope":"apk_preflight","checks":checks,"facts":safe_facts,
                "certificate_binding_checked":data["certificate_binding_checked"].as_bool().ok_or("CLI_PROTOCOL_INVALID")?})
        }
        _ => return Err("CLI_PROTOCOL_INVALID"),
    };
    Ok(json!({"schema_version":1,"operation":operation,"status":status,"data":safe}))
}

fn controlled(events: &[Value], operation: &str, exit: i32) -> Result<Value> {
    let last = events.last().ok_or("CLI_PROTOCOL_INVALID")?;
    let mut stream = Stream::new(operation);
    for event in events {
        stream.accept(event)?;
    }
    if !stream.ended {
        return Err("CLI_PROTOCOL_INVALID");
    }
    let plan_only = operation == "agent_plan";
    let signing = operation == "agent_sign";
    let status = last["status"].as_str().ok_or("CLI_PROTOCOL_INVALID")?;
    let expected = match status {
        "ready" | "warning" if plan_only => 0,
        "succeeded" if !plan_only => 0,
        "blocked" | "failed" => 1,
        "unknown" => 3,
        "timed_out" => 124,
        "cancelled" => 130,
        _ => return Err("CLI_PROTOCOL_INVALID"),
    };
    let cleanup = last["cleanup_state"]
        .as_str()
        .ok_or("CLI_PROTOCOL_INVALID")?;
    if exit != expected
        || last["retryable"] != false
        || ![
            "not_needed",
            "complete",
            "incomplete",
            "retained",
            "unconfirmed",
        ]
        .contains(&cleanup)
    {
        return Err("CLI_PROTOCOL_INVALID");
    }
    let output = last["output_state"]
        .as_str()
        .ok_or("CLI_PROTOCOL_INVALID")?;
    if plan_only && output != "not_published" {
        return Err("CLI_PROTOCOL_INVALID");
    }
    let result_code = code(&last["code"])?;
    if !matches!(status, "ready" | "warning" | "succeeded") && result_code == "OK" {
        return Err("CLI_PROTOCOL_INVALID");
    }
    let mut safe = json!({"schema_version":1,"operation":operation,"status":status,
        "code":result_code,"output_state":output,"cleanup_state":cleanup,"retryable":false});
    match output {
        "not_published" if status != "succeeded" => {
            if !last["artifact"].is_null() || !last["directory_synced"].is_null() {
                return Err("CLI_PROTOCOL_INVALID");
            }
        }
        "uncertain" if status == "unknown" && cleanup != "complete" => {
            if !last["artifact"].is_null() || !last["directory_synced"].is_null() {
                return Err("CLI_PROTOCOL_INVALID");
            }
        }
        "published" if !plan_only && matches!(status, "succeeded" | "unknown") => {
            let artifact = &last["artifact"];
            let bytes = number(&artifact["bytes"])?;
            let digest = artifact["sha256"].as_str().ok_or("CLI_PROTOCOL_INVALID")?;
            if bytes == 0 || !hex_digest(digest) {
                return Err("CLI_PROTOCOL_INVALID");
            }
            safe["artifact"] = json!({"bytes":bytes,"sha256":digest});
            safe["directory_synced"] = json!(last["directory_synced"]
                .as_bool()
                .ok_or("CLI_PROTOCOL_INVALID")?);
        }
        _ => return Err("CLI_PROTOCOL_INVALID"),
    }
    if !last["plan"].is_null() {
        let plan = &last["plan"];
        let digest = plan["sha256"].as_str().ok_or("CLI_PROTOCOL_INVALID")?;
        let key_checked = plan["private_key_access_checked"]
            .as_bool()
            .ok_or("CLI_PROTOCOL_INVALID")?;
        let version_valid = if signing {
            plan["schema_version"] == 4
        } else {
            matches!(plan["schema_version"].as_u64(), Some(2 | 3))
        };
        if !version_valid || !hex_digest(digest) || key_checked != (output == "published") {
            return Err("CLI_PROTOCOL_INVALID");
        }
        safe["plan"] = json!({"schema_version":plan["schema_version"],"sha256":digest,"private_key_access_checked":key_checked});
        if plan["schema_version"] == 3 {
            let binding = plan["shared_binding"]
                .as_str()
                .ok_or("CLI_PROTOCOL_INVALID")?;
            crate::automation::execution::shared_binding_certificate(binding)?;
            // 仅供会话私有缓存；runner 在形成对外结果前移除此字段。
            safe["plan"]["shared_binding"] = json!(binding);
        } else if !plan["shared_binding"].is_null() {
            return Err("CLI_PROTOCOL_INVALID");
        }
    }
    if matches!(status, "ready" | "warning" | "succeeded")
        && (cleanup != "complete" || result_code != "OK" || safe["plan"].is_null())
    {
        return Err("CLI_PROTOCOL_INVALID");
    }
    // Windows 不提供目录同步保证；如实保留 false，不抹掉已确认交付的事实。
    if status == "succeeded" && !cfg!(windows) && safe["directory_synced"] != true {
        return Err("CLI_PROTOCOL_INVALID");
    }
    if output == "published" && safe["plan"].is_null() {
        return Err("CLI_PROTOCOL_INVALID");
    }
    if !last["warnings"].is_null() {
        if !signing || last["warnings"] != json!(["SIGNING_CERTIFICATE_CHANGED"]) {
            return Err("CLI_PROTOCOL_INVALID");
        }
        safe["warnings"] = last["warnings"].clone();
    }
    Ok(safe)
}

const STAGES: [&str; 13] = [
    "prepare",
    "check_tools",
    "preflight",
    "validate_resources",
    "unpack",
    "modify_manifest",
    "process_dex",
    "inject_runtime",
    "repack",
    "align",
    "sign",
    "verify",
    "publish",
];
#[derive(Clone)]
pub(super) struct Progress {
    pub stage: &'static str,
    pub index: u8,
    pub plan_id: Option<String>,
}
pub(super) struct Stream<'a> {
    operation: &'a str,
    next: u64,
    task: Value,
    last_stage: Option<usize>,
    ended: bool,
}
impl<'a> Stream<'a> {
    pub fn new(operation: &'a str) -> Self {
        Self {
            operation,
            next: 0,
            task: Value::Null,
            last_stage: None,
            ended: false,
        }
    }
    pub fn accept(&mut self, event: &Value) -> Result<Option<Progress>> {
        if self.ended
            || self.next >= 32
            || event["schema_version"] != 1
            || event["experimental"] != true
            || event["operation"] != self.operation
            || event["tool_version"] != env!("CARGO_PKG_VERSION")
            || event["sequence"] != self.next
        {
            return Err("CLI_PROTOCOL_INVALID");
        }
        if self.next == 0 {
            self.task = event["task_id"].clone();
            let task_valid = self.task.as_str().is_some_and(|value| {
                value.len() == 32 && value.bytes().all(|c| c.is_ascii_hexdigit())
            });
            if !(task_valid && event["type"] == "started"
                || self.task.is_null() && event["type"] == "result")
            {
                return Err("CLI_PROTOCOL_INVALID");
            }
        } else if event["task_id"] != self.task {
            return Err("CLI_PROTOCOL_INVALID");
        }
        self.next += 1;
        match event["type"].as_str() {
            Some("started") if self.next == 1 => Ok(None),
            Some("result") => {
                self.ended = true;
                Ok(None)
            }
            Some("progress") if self.next > 1 => {
                let index = STAGES
                    .iter()
                    .position(|stage| event["stage"] == *stage)
                    .ok_or("CLI_PROTOCOL_INVALID")?;
                if self.last_stage.is_some_and(|before| before >= index)
                    || self.operation == "agent_plan" && index > 3
                    || self.operation == "agent_sign" && ![0, 1, 9, 10, 11, 12].contains(&index)
                {
                    return Err("CLI_PROTOCOL_INVALID");
                }
                number(&event["elapsed_ms"])?;
                self.last_stage = Some(index);
                Ok(Some(Progress {
                    stage: STAGES[index],
                    index: index as u8,
                    plan_id: None,
                }))
            }
            _ => Err("CLI_PROTOCOL_INVALID"),
        }
    }
}

pub(super) fn hex_digest(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|c| c.is_ascii_hexdigit())
}
fn checks(value: &Value, dependency: bool) -> Result<Vec<Value>> {
    let list = value.as_array().ok_or("CLI_PROTOCOL_INVALID")?;
    if list.len() > 64 {
        return Err("CLI_PROTOCOL_INVALID");
    }
    list.iter()
        .map(|check| {
            let mut safe = json!({"code":code(&check["code"])?,"status":status(&check["status"])?});
            if dependency {
                let name = check["dependency"].as_str().ok_or("CLI_PROTOCOL_INVALID")?;
                if !["java", "keytool", "apktool", "apksigner", "resources"].contains(&name) {
                    return Err("CLI_PROTOCOL_INVALID");
                }
                safe["dependency"] = json!(name);
            }
            Ok(safe)
        })
        .collect()
}
fn status(value: &Value) -> Result<&str> {
    let value = value.as_str().ok_or("CLI_PROTOCOL_INVALID")?;
    if ["ready", "warning", "blocked"].contains(&value) {
        Ok(value)
    } else {
        Err("CLI_PROTOCOL_INVALID")
    }
}
fn code(value: &Value) -> Result<&str> {
    let value = value.as_str().ok_or("CLI_PROTOCOL_INVALID")?;
    if !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'_')
    {
        Ok(value)
    } else {
        Err("CLI_PROTOCOL_INVALID")
    }
}
fn number(value: &Value) -> Result<u64> {
    value.as_u64().ok_or("CLI_PROTOCOL_INVALID")
}

#[cfg(test)]
#[path = "protocol/signing_tests.rs"]
mod signing_tests;
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn 证书摘要只投影编号默认和版本不转发自由字段() {
        let row = json!({"id":"allowed","is_default":true,"sign_v1":true,"sign_v2":true,"sign_v3":true,"sign_v4":false,
            "name":"private-name","keystore_path":"private-path","key_alias":"private-alias","note":"private-note","password":"private-password"});
        let event = json!({"schema_version":1,"tool_version":env!("CARGO_PKG_VERSION"),"operation":"cert","type":"result","status":"ready","data":{"certificates":[row]}});
        let decoded = decode(format!("{event}\n").as_bytes(), "cert", 0).unwrap();
        assert_eq!(
            decoded["data"]["certificates"][0]
                .as_object()
                .unwrap()
                .len(),
            3
        );
        assert!(!decoded.to_string().contains("private-"));
        let mut bad = event;
        bad["data"]["certificates"][0]["sign_v1"] = json!("true");
        assert!(decode(format!("{bad}\n").as_bytes(), "cert", 0).is_err());
    }
    #[test]
    fn 不转发路径或工具原文且拒绝重复终态与错误退出码() {
        let value = json!({"schema_version":1,"tool_version":env!("CARGO_PKG_VERSION"),"operation":"doctor",
            "type":"result","status":"ready","message":"秘密路径","data":{"scope":"dependency_presence","java_major":17,"checks":[]}});
        let bytes = format!("{value}\n");
        assert!(!decode(bytes.as_bytes(), "doctor", 0)
            .unwrap()
            .to_string()
            .contains("秘密路径"));
        assert!(decode(bytes.as_bytes(), "doctor", 1).is_err());
        assert!(decode(format!("{bytes}{bytes}").as_bytes(), "doctor", 0).is_err());
        assert!(decode(value.to_string().as_bytes(), "doctor", 0).is_err());
        assert!(decode("原始工具输出\n".as_bytes(), "doctor", 0).is_err());
    }

    #[test]
    fn 计划必须具有唯一连续终态且不能包含已交付或访问过私钥的矛盾证据() {
        let header = |sequence, kind| {
            json!({"schema_version":1,"tool_version":env!("CARGO_PKG_VERSION"),
            "operation":"agent_plan","experimental":true,"sequence":sequence,"type":kind,"task_id":"a".repeat(32)})
        };
        let started = header(0, "started");
        let mut terminal = header(1, "result");
        terminal["status"] = json!("ready");
        terminal["code"] = json!("OK");
        terminal["retryable"] = json!(false);
        terminal["cleanup_state"] = json!("complete");
        terminal["output_state"] = json!("not_published");
        terminal["plan"] =
            json!({"schema_version":2,"sha256":"b".repeat(64),"private_key_access_checked":false});
        let encode = |events: &[Value]| events.iter().map(|v| format!("{v}\n")).collect::<String>();
        let valid = vec![started.clone(), terminal.clone()];
        assert!(decode(encode(&valid).as_bytes(), "agent_plan", 0).is_ok());
        for (field, value) in [
            ("sequence", json!(3)),
            ("output_state", json!("published")),
            ("cleanup_state", json!("unconfirmed")),
            ("retryable", json!(true)),
            ("code", json!("原始异常")),
        ] {
            let mut changed = terminal.clone();
            changed[field] = value;
            assert!(decode(
                encode(&[started.clone(), changed]).as_bytes(),
                "agent_plan",
                0
            )
            .is_err());
        }
        let mut private = terminal.clone();
        private["plan"]["private_key_access_checked"] = json!(true);
        assert!(decode(
            encode(&[started.clone(), private]).as_bytes(),
            "agent_plan",
            0
        )
        .is_err());
        assert!(decode(
            encode(&[started, terminal.clone(), terminal]).as_bytes(),
            "agent_plan",
            0
        )
        .is_err());
        assert!(decode(encode(&valid).as_bytes(), "agent_plan", 1).is_err());
    }

    #[test]
    fn 执行回执必须同时具有交付证据且拒绝截断或重复终态() {
        let started = json!({"schema_version":1,"tool_version":env!("CARGO_PKG_VERSION"),"operation":"agent_protect",
            "experimental":true,"sequence":0,"type":"started","task_id":"a".repeat(32)});
        let mut result = started.clone();
        result["sequence"] = json!(1);
        result["type"] = json!("result");
        result["status"] = json!("succeeded");
        result["code"] = json!("OK");
        result["retryable"] = json!(false);
        result["output_state"] = json!("published");
        result["cleanup_state"] = json!("complete");
        result["directory_synced"] = json!(true);
        result["artifact"] = json!({"bytes":42,"sha256":"b".repeat(64)});
        result["plan"] =
            json!({"schema_version":2,"sha256":"c".repeat(64),"private_key_access_checked":true});
        let encode = |last: &Value| format!("{started}\n{last}\n");
        assert_eq!(
            decode(encode(&result).as_bytes(), "agent_protect", 0).unwrap()["output_state"],
            "published"
        );
        for (field, value) in [
            ("artifact", Value::Null),
            ("plan", Value::Null),
            ("output_state", json!("not_published")),
            ("status", json!("cancelled")),
            ("directory_synced", Value::Null),
        ] {
            let mut bad = result.clone();
            bad[field] = value;
            assert!(decode(encode(&bad).as_bytes(), "agent_protect", 0).is_err());
        }
        let mut unsynced = result.clone();
        unsynced["directory_synced"] = json!(false);
        let decoded = decode(encode(&unsynced).as_bytes(), "agent_protect", 0);
        assert_eq!(decoded.is_ok(), cfg!(windows));
        #[cfg(windows)]
        assert_eq!(decoded.unwrap()["directory_synced"], false);
        assert!(decode(format!("{started}\n").as_bytes(), "agent_protect", 0).is_err());
        assert!(decode(
            format!("{}{result}\n", encode(&result)).as_bytes(),
            "agent_protect",
            0
        )
        .is_err());
        assert!(decode(encode(&result).as_bytes(), "agent_protect", 130).is_err());
    }
}

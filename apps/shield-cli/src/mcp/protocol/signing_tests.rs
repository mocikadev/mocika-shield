use super::*;
fn event(sequence: u64, kind: &str) -> Value {
    json!({"schema_version":1,"tool_version":env!("CARGO_PKG_VERSION"),"operation":"agent_sign", "experimental":true,
        "sequence":sequence,"type":kind,"task_id":"a".repeat(32)})
}
#[test]
fn 签名仅接收独立计划版本和固定警告且不透传私有字段() {
    let start = event(0, "started");
    let mut result = event(1, "result");
    result["status"] = json!("succeeded");
    result["code"] = json!("OK");
    result["retryable"] = json!(false);
    result["output_state"] = json!("published");
    result["cleanup_state"] = json!("complete");
    result["directory_synced"] = json!(true);
    result["artifact"] = json!({"bytes":42,"sha256":"b".repeat(64)});
    result["plan"] =
        json!({"schema_version":4,"sha256":"c".repeat(64),"private_key_access_checked":true});
    result["message"] = json!("不能发送的私有路径");
    result["warnings"] = json!(["SIGNING_CERTIFICATE_CHANGED"]);
    let encode = |last: &Value| format!("{start}\n{last}\n");
    let value = decode(encode(&result).as_bytes(), "agent_sign", 0).unwrap();
    assert_eq!(value["plan"]["schema_version"], 4);
    assert_eq!(value["warnings"], json!(["SIGNING_CERTIFICATE_CHANGED"]));
    assert!(!value.to_string().contains("私有路径"));
    for version in [2, 3, 5] {
        let mut changed = result.clone();
        changed["plan"]["schema_version"] = json!(version);
        assert!(decode(encode(&changed).as_bytes(), "agent_sign", 0).is_err());
    }
    for warning in [
        json!(["私有原始日志"]),
        json!(["SIGNING_CERTIFICATE_CHANGED", "SIGNING_CERTIFICATE_CHANGED"]),
        json!([]),
    ] {
        let mut changed = result.clone();
        changed["warnings"] = warning;
        assert!(decode(encode(&changed).as_bytes(), "agent_sign", 0).is_err());
    }
    let mut changed = result.clone();
    changed["plan"]["shared_binding"] = json!("不可接受");
    assert!(decode(encode(&changed).as_bytes(), "agent_sign", 0).is_err());
    let mut old_start = start;
    old_start["operation"] = json!("agent_protect");
    result["operation"] = json!("agent_protect");
    result.as_object_mut().unwrap().remove("warnings");
    assert!(decode(
        format!("{old_start}\n{result}\n").as_bytes(),
        "agent_protect",
        0
    )
    .is_err());
}
#[test]
fn 签名进度不接受加固阶段或重复倒退() {
    for stage in [
        "unpack",
        "process_dex",
        "inject_runtime",
        "validate_resources",
    ] {
        let mut stream = Stream::new("agent_sign");
        stream.accept(&event(0, "started")).unwrap();
        let mut progress = event(1, "progress");
        progress["stage"] = json!(stage);
        progress["elapsed_ms"] = json!(1);
        assert!(stream.accept(&progress).is_err());
    }
    let mut stream = Stream::new("agent_sign");
    stream.accept(&event(0, "started")).unwrap();
    for (index, stage) in [
        "prepare",
        "check_tools",
        "align",
        "sign",
        "verify",
        "publish",
    ]
    .iter()
    .enumerate()
    {
        let mut progress = event(index as u64 + 1, "progress");
        progress["stage"] = json!(stage);
        progress["elapsed_ms"] = json!(1);
        assert_eq!(stream.accept(&progress).unwrap().unwrap().stage, *stage);
    }
    let mut repeated = event(7, "progress");
    repeated["stage"] = json!("sign");
    repeated["elapsed_ms"] = json!(2);
    assert!(stream.accept(&repeated).is_err());
}

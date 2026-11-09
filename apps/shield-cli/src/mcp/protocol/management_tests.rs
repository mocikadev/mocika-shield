use super::*;
fn response(kind: Management, data: Value) -> Value {
    json!({"schema_version":1,"tool_version":env!("CARGO_PKG_VERSION"),"operation":kind.operation(),"type":"result","status":"ready","data":data})
}
fn parse(value: &Value, kind: Management, exit: i32) -> Result<Value> {
    decode_management(format!("{value}\n").as_bytes(), kind, exit)
}
#[test]
fn 设置只返回固定业务字段而不复制原配置或路径() {
    let data = json!({"protect_defaults":{"runtime_mode":"android_api19","environment_policy":"strict","sign_after_protect":null,"certificate_id":"allowed","fixed_output_directory":"private-path","output_directory_mode":"fixed"},"telemetry":"private-data"});
    let event = response(Management::Settings, data);
    let safe = parse(&event, Management::Settings, 0).unwrap();
    assert_eq!(
        safe["data"]["protect_defaults"].as_object().unwrap().len(),
        4
    );
    assert!(!safe.to_string().contains("private-"));
    for key in ["runtime_mode", "sign_after_protect", "certificate_id"] {
        let mut bad = event.clone();
        bad["data"]["protect_defaults"]
            .as_object_mut()
            .unwrap()
            .remove(key);
        assert!(parse(&bad, Management::Settings, 0).is_err());
    }
    assert!(parse(&event, Management::Settings, 1).is_err());
}
#[test]
fn 写入回执不泄漏自由字段并拒绝矛盾或异常状态() {
    let event = response(
        Management::UpdateCertificate,
        json!({"certificate":{"id":"allowed","is_default":true,"sign_v1":true,"sign_v2":true,"sign_v3":false,"sign_v4":false,"auto_sign_enabled":false,"name":"private-name","note":"private-note","keystore_path":"private-path","key_alias":"private-alias","keystore_password":"private-password"}}),
    );
    let safe = parse(&event, Management::UpdateCertificate, 0).unwrap();
    assert_eq!(safe["change_state"], "confirmed");
    assert!(!safe.to_string().contains("private-"));
    for key in ["id", "is_default", "sign_v1", "auto_sign_enabled"] {
        let mut bad = event.clone();
        bad["data"]["certificate"][key] = json!(17);
        assert!(parse(&bad, Management::UpdateCertificate, 0).is_err());
    }
    assert!(decode_management(
        format!("{event}\n{event}\n").as_bytes(),
        Management::UpdateCertificate,
        0
    )
    .is_err());
}
#[test]
fn 修改失败保留未尝试或未确认而不转发异常原文() {
    for state in ["not_attempted", "unconfirmed"] {
        let value = json!({"schema_version":1,"tool_version":env!("CARGO_PKG_VERSION"),"operation":"config","type":"error","code":"CONFIG_OPERATION_FAILED","change_state":state,"message":"private-password"});
        let safe = parse(&value, Management::UpdateSettings, 1).unwrap();
        assert_eq!(safe["change_state"], state);
        assert_eq!(
            safe["status"],
            if state == "unconfirmed" {
                "unknown"
            } else {
                "blocked"
            }
        );
        assert!(!safe.to_string().contains("private-"));
        assert!(parse(&value, Management::UpdateSettings, 0).is_err());
        let mut missing = value;
        missing.as_object_mut().unwrap().remove("change_state");
        assert!(parse(&missing, Management::UpdateSettings, 1).is_err());
    }
}

#[test]
fn 校验失败但保存确认与删除保留事实独立于成功状态() {
    let value = json!({"schema_version":1,"tool_version":env!("CARGO_PKG_VERSION"),"operation":"cert","type":"error","code":"KEYSTORE_PASSWORD_INVALID","change_state":"confirmed","data":{"certificate":{"id":"allowed","verification_status":"failed","message":"private-message"}}});
    let safe = parse(&value, Management::Verify, 1).unwrap();
    assert_eq!(safe["status"], "blocked");
    assert_eq!(safe["change_state"], "confirmed");
    assert!(!safe.to_string().contains("private-"));
    assert!(parse(&value, Management::Create, 1).is_err());
    let mut bad = value.clone();
    bad["data"]["certificate"]["verification_status"] = json!("success");
    assert!(parse(&bad, Management::Verify, 1).is_err());
    let removed = response(
        Management::Remove,
        json!({"removed_id":"allowed","keystore_file":"retained","path":"private-path"}),
    );
    assert_eq!(
        parse(&removed, Management::Remove, 0).unwrap()["data"]["keystore_file"],
        "retained"
    );
    let mut bad = removed;
    bad["data"]["keystore_file"] = json!("deleted");
    assert!(parse(&bad, Management::Remove, 0).is_err());
}

use super::*;
use serde_json::{json, Value};

fn reply(operation: &str, result: Value) -> Value {
    json!({"protocol_version":PROTOCOL,"application_version":env!("CARGO_PKG_VERSION"),"operation":operation,"result":result})
}

#[test]
fn 只读结果严格绑定操作版本状态及字段() {
    let valid = reply(ALIASES, json!({"aliases":["中文,引号\"😀"]}));
    let bytes = serde_json::to_vec(&valid).unwrap();
    assert_eq!(
        decode::<Aliases>(&bytes, Some(0), ALIASES).unwrap().aliases,
        ["中文,引号\"😀"]
    );
    for status in [None, Some(2), Some(4)] {
        assert!(decode::<Aliases>(&bytes, status, ALIASES).is_err());
    }
    assert!(decode::<Aliases>(&bytes, Some(0), FINGERPRINT).is_err());
    for (key, value) in [
        ("protocol_version", json!(PROTOCOL - 1)),
        ("application_version", json!("0.0.0")),
        ("extra", Value::Null),
        ("error", json!("PASSWORD_INVALID")),
        ("password", json!("不得泄露的口令")),
    ] {
        let mut invalid = valid.clone();
        invalid[key] = value;
        let error = decode::<Aliases>(&serde_json::to_vec(&invalid).unwrap(), Some(0), ALIASES)
            .err()
            .unwrap();
        assert_eq!(
            Diagnostic::from_error(&error).code,
            FailureCode::ToolProcessFailed
        );
        assert!(!error.to_string().contains("不得泄露"));
    }
    for body in [
        json!({"aliases":null}),
        json!({"aliases":[],"fingerprint":null}),
        json!({"aliases":[1]}),
    ] {
        assert!(decode::<Aliases>(
            &serde_json::to_vec(&reply(ALIASES, body)).unwrap(),
            Some(0),
            ALIASES
        )
        .is_err());
    }
    assert!(decode::<Aliases>(&vec![b' '; MAX as usize + 1], Some(0), ALIASES).is_err());
    let duplicate = String::from_utf8(bytes).unwrap().replacen(
        "\"protocol_version\":",
        "\"protocol_version\":2,\"protocol_version\":",
        1,
    );
    assert!(decode::<Aliases>(duplicate.as_bytes(), Some(0), ALIASES).is_err());
}

#[test]
fn 固定错误映射不暴露原始输入且失败不能伪装成功() {
    for (name, code) in [
        ("PASSWORD_INVALID", FailureCode::KeystorePasswordInvalid),
        ("ALIAS_NOT_FOUND", FailureCode::KeyAliasNotFound),
        ("FORMAT_INVALID", FailureCode::KeystoreFormatInvalid),
        ("FILE_NOT_FOUND", FailureCode::FileNotFound),
        ("PERMISSION_DENIED", FailureCode::PermissionDenied),
        ("CERTIFICATE_MISSING", FailureCode::ToolProcessFailed),
        ("RESULT_LIMIT", FailureCode::ToolProcessFailed),
        ("TOOL_FAILED", FailureCode::ToolProcessFailed),
        ("INPUT_INVALID", FailureCode::Unknown),
        ("SUBJECT_INVALID", FailureCode::Unknown),
        ("PASSWORD_UNSUPPORTED", FailureCode::KeystorePasswordInvalid),
        ("OUTPUT_EXISTS", FailureCode::ToolProcessFailed),
        ("UNKNOWN", FailureCode::ToolProcessFailed),
    ] {
        let bytes=serde_json::to_vec(&json!({"protocol_version":PROTOCOL,"application_version":env!("CARGO_PKG_VERSION"),"operation":ALIASES,"error":name})).unwrap();
        let error = decode::<Aliases>(&bytes, Some(4), ALIASES).err().unwrap();
        let actual = Diagnostic::from_error(&error);
        assert_eq!(actual.code, code);
        assert_eq!(actual.tool, Some(ToolName::Keytool));
        assert!(decode::<Aliases>(&bytes, Some(0), ALIASES).is_err());
    }
}

#[test]
fn 完整别名预算去重及指纹格式不能放宽() {
    for values in [
        vec![],
        vec!["same".into(), "same".into()],
        vec!["x".repeat(4097)],
        (0..257).map(|n| n.to_string()).collect(),
    ] {
        assert!(aliases(Aliases { aliases: values }).is_err());
    }
    assert_eq!(
        aliases(Aliases {
            aliases: vec!["中文😀".into()]
        })
        .unwrap(),
        ["中文😀"]
    );
    for value in ["a".repeat(64), "G".repeat(64), "A".repeat(63)] {
        assert!(fingerprint(Fingerprint { fingerprint: value }).is_err());
    }
    assert_eq!(
        fingerprint(Fingerprint {
            fingerprint: "A".repeat(64)
        })
        .unwrap(),
        "A".repeat(64)
    );
}

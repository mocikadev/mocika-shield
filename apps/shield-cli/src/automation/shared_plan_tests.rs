use super::*;
use serde_json::json;

fn fixture() -> Binding {
    let digest = [0u8; 32];
    serde_json::from_value(json!({"version":1,"certificate_binding":{
        "schema_version":1,"id":"synthetic-id","state_sha256":digest},
        "runtime_profile":"android-api19","environment_policy":"strict",
        "signing_versions":[true,true,true,false]}))
    .ok()
    .unwrap()
}
fn encode(value: serde_json::Value) -> String {
    serde_json::to_vec(&value)
        .unwrap()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
#[test]
fn 绑定往返只携带安全字段且不接受非规范编码() {
    let binding = fixture();
    let text = binding.encode().ok().unwrap();
    assert!(Binding::decode(&text).ok().unwrap() == binding);
    for invalid in [
        String::new(),
        "0".into(),
        "zz".into(),
        text.to_uppercase(),
        "ab".repeat(TOKEN_LIMIT),
    ] {
        assert!(Binding::decode(&invalid).is_err());
    }
    let mut bytes = serde_json::to_vec(&binding).unwrap();
    bytes.push(b' ');
    let padded: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    assert!(Binding::decode(&padded).is_err());
}
#[test]
fn 绑定拒绝未知字段版本枚举无签名和附加产物() {
    let original = serde_json::to_value(fixture()).unwrap();
    for (name, value) in [
        ("version", json!(2)),
        ("runtime_profile", json!("other")),
        ("environment_policy", json!("other")),
        ("signing_versions", json!([false, false, false, false])),
        ("signing_versions", json!([true, true, true, true])),
        ("password", json!("must-not-return")),
    ] {
        let mut modified = original.clone();
        modified[name] = value;
        assert!(Binding::decode(&encode(modified)).is_err());
    }
    let mut modified = original;
    modified["certificate_binding"]["schema_version"] = json!(2);
    assert!(Binding::decode(&encode(modified)).is_err());
}
#[test]
fn 执行绑定不得与重新选择证书或策略混用() {
    let temp = tempfile::tempdir().unwrap();
    let mut request = SharedRequest {
        app_home: Some(temp.path().to_path_buf()),
        certificate_id: None,
        use_default_certificate: false,
        runtime_profile: None,
        environment_policy: None,
        binding: Some(fixture()),
    };
    assert!(request.validate().is_ok());
    request.use_default_certificate = true;
    assert!(request.validate().is_err());
    request.use_default_certificate = false;
    request.runtime_profile = Some(Profile::Standard);
    assert!(request.validate().is_err());
    assert!(!temp.path().join("data").exists());
}

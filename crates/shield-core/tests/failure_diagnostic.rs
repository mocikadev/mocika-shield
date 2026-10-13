use shield_core::diagnostic::{Diagnostic, FailureCode, ToolName};
use shield_core::ShieldError;

#[test]
fn 类型错误不携带敏感文本() {
    let error = ShieldError::ToolNotFound("/Users/private/secret".into());
    let diagnostic = Diagnostic::from_shield(&error).unwrap();
    assert_eq!(diagnostic.code, FailureCode::ToolNotFound);
    assert!(!format!("{diagnostic:?}").contains("secret"));
    assert!(Diagnostic::from_shield(&ShieldError::Cancelled).is_none());
}

#[test]
fn 不对任意文本猜测错误原因() {
    let error = anyhow::anyhow!("/private/password was incorrect.apk");
    assert_eq!(Diagnostic::from_error(&error).code, FailureCode::Unknown);
    let io = anyhow::Error::new(std::io::Error::from(std::io::ErrorKind::PermissionDenied));
    assert_eq!(
        Diagnostic::from_error(&io).code,
        FailureCode::PermissionDenied
    );
}

#[test]
fn 工具范围分类与无效编码保持有限输出() {
    let result = Diagnostic::from_tool_output(
        ToolName::Keytool,
        Some(1),
        b"keytool error: java.io.IOException: keystore password was incorrect",
    );
    assert_eq!(result.code, FailureCode::KeystorePasswordInvalid);
    assert!(!result.code.should_prompt());
    let invalid = Diagnostic::from_tool_output(ToolName::Keytool, Some(1), b"\xff\xfe");
    assert_eq!(invalid.code, FailureCode::ToolOutputEncodingInvalid);
    let unrelated =
        Diagnostic::from_tool_output(ToolName::Apktool, Some(1), b"password was incorrect");
    assert_eq!(unrelated.code, FailureCode::ToolProcessFailed);
}

#[test]
fn 诊断上下文不改变原错误显示() {
    let error = anyhow::anyhow!("原始本地错误");
    let error = Diagnostic::new(FailureCode::ApkRepackFailed).attach(error);
    assert_eq!(error.to_string(), "原始本地错误");
    assert_eq!(
        Diagnostic::from_error(&error).code,
        FailureCode::ApkRepackFailed
    );
}

#[test]
fn 标准错误提示不掩盖标准输出证据并对重复去重() {
    let diagnostic = Diagnostic::from_tool_streams(
        ToolName::Apktool,
        Some(1),
        b"error: resource private not found\nfailed linking resources",
        b"Picked up JAVA_TOOL_OPTIONS: secret\nfailed linking resources",
    );
    let evidence: Vec<_> = diagnostic.evidence.into_iter().flatten().collect();
    assert_eq!(evidence.len(), 2);
    assert!(evidence
        .iter()
        .any(|item| item.kind == "resource_not_found"));
    assert!(!format!("{diagnostic:?}").contains("private"));
    assert!(!format!("{diagnostic:?}").contains("secret"));
}

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

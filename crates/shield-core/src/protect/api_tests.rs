use super::{native_library_packaging, validate_apk_eligibility, validate_output_certificate};
use crate::diagnostic::{Diagnostic, FailureCode, ToolName};

#[test]
fn apktool_失败在字节转换前保留工具诊断() {
    let unsupported = super::toolchain::apktool_command_error(
        std::path::Path::new("java"),
        FailureCode::ApkRepackFailed,
        Some(1),
        b"",
        b"java.lang.UnsupportedClassVersionError",
    );
    assert_eq!(
        Diagnostic::from_error(&unsupported).code,
        FailureCode::JavaUnsupported
    );
    assert_eq!(
        Diagnostic::from_error(&unsupported).tool,
        Some(ToolName::Apktool)
    );
    assert_eq!(Diagnostic::from_error(&unsupported).exit_code, Some(1));
    assert!(unsupported
        .to_string()
        .contains("UnsupportedClassVersionError"));

    let invalid = super::toolchain::apktool_command_error(
        std::path::Path::new("java"),
        FailureCode::ApkRepackFailed,
        Some(1),
        b"",
        b"\xff\xfe",
    );
    assert_eq!(
        Diagnostic::from_error(&invalid).code,
        FailureCode::ToolOutputEncodingInvalid
    );
    assert!(invalid.to_string().contains("命令执行失败"));

    let unpack = super::toolchain::apktool_command_error(
        std::path::Path::new("java"),
        FailureCode::ToolProcessFailed,
        Some(2),
        b"",
        b"ordinary apktool failure",
    );
    let repack = super::toolchain::apktool_command_error(
        std::path::Path::new("java"),
        FailureCode::ApkRepackFailed,
        Some(3),
        b"",
        b"ordinary apktool failure",
    );
    assert_eq!(
        Diagnostic::from_error(&unpack).code,
        FailureCode::ToolProcessFailed
    );
    assert_eq!(
        Diagnostic::from_error(&repack).code,
        FailureCode::ApkRepackFailed
    );
}
use crate::apk_inspect::ApkCheckOutcome;
use crate::protect::manifest::NativeLibPackagingPolicy;
use crate::zipalign::NativeLibraryPackaging;

#[test]
fn extract_native_libs_false_映射为不压缩策略() {
    assert_eq!(
        native_library_packaging(NativeLibPackagingPolicy::Disabled),
        NativeLibraryPackaging::Store
    );
    assert_eq!(
        native_library_packaging(NativeLibPackagingPolicy::Enabled),
        NativeLibraryPackaging::Preserve
    );
    assert_eq!(
        native_library_packaging(NativeLibPackagingPolicy::Unspecified),
        NativeLibraryPackaging::Preserve
    );
}

#[test]
fn 已加固_apk_被核心入口拒绝() {
    let outcome = ApkCheckOutcome {
        already_protected: true,
        is_signed: true,
        native_abis: Vec::new(),
    };
    let error = validate_apk_eligibility(&outcome).unwrap_err();
    assert!(error.to_string().contains("禁止重复加固"));
}

#[test]
fn 未签名_apk_被核心入口拒绝() {
    let outcome = ApkCheckOutcome {
        already_protected: false,
        is_signed: false,
        native_abis: Vec::new(),
    };
    let error = validate_apk_eligibility(&outcome).unwrap_err();
    assert!(error.to_string().contains("尚未签名"));
}

#[test]
fn 自动签名证书不一致时失败关闭() {
    let error = validate_output_certificate("AA:BB", Some("CCDD")).unwrap_err();
    assert!(error.to_string().contains("应用将无法启动"));
}

#[test]
fn 自动签名证书比较会规范化格式() {
    validate_output_certificate("aa:bb cc", Some("AABBCC")).unwrap();
}

#[test]
fn 未配置自动签名时允许生成未签名产物() {
    validate_output_certificate("AABB", None).unwrap();
}

use super::{classify_keytool_error, parse_keytool_aliases};

#[test]
fn keytool_密码错误提示更明确() {
    let message = classify_keytool_error(
        "keytool error: java.io.IOException: keystore password was incorrect",
    );
    assert!(message.starts_with("Keystore 密码不正确"));
}

#[test]
fn keytool_旧版密码错误提示更明确() {
    let message = classify_keytool_error(
        "keytool error: java.io.IOException: Keystore was tampered with, or password was incorrect",
    );
    assert!(message.starts_with("Keystore 密码不正确"));
}

#[test]
fn keytool_格式不匹配提示更明确() {
    let message =
        classify_keytool_error("keytool error: java.io.IOException: Invalid keystore format");
    assert!(message.starts_with("证书格式可能不是当前选择的 JKS/PKCS12"));
}

#[test]
fn keytool_文件不存在提示更明确() {
    let message =
        classify_keytool_error("keytool error: java.io.FileNotFoundException: no such file");
    assert!(message.starts_with("找不到 Keystore 文件"));
}

#[test]
fn keytool_未知错误不回传原始工具输出() {
    let message = classify_keytool_error("keytool error: unknown failure");
    assert!(message.starts_with("无法读取证书文件"));
    assert!(!message.contains("unknown failure"));
}

#[test]
fn 解析_keytool_alias_保持原有行为() {
    let output = "\
release, 2026年7月9日, PrivateKeyEntry,
trusted, 2026年7月9日, trustedCertEntry,";
    assert_eq!(
        parse_keytool_aliases(output.as_bytes()).unwrap(),
        vec!["release", "trusted"]
    );
}

#[test]
fn 中文_alias_按_utf8_完整保留() {
    let output = "中文签名pos, Sep 16, 2026, PrivateKeyEntry,";
    assert_eq!(
        parse_keytool_aliases(output.as_bytes()).unwrap(),
        vec!["中文签名pos"]
    );
}

#[test]
fn 非_utf8_alias_不得替换成乱码后返回() {
    let output = b"\xd6\xd0\xce\xc4pos, Sep 16, 2026, PrivateKeyEntry,";
    assert!(parse_keytool_aliases(output).unwrap_err().contains("UTF-8"));
}

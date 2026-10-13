//! 工具输出的白名单证据：只返回固定模板，不保存原文或动态参数。
use crate::diagnostic::ToolName;

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
pub struct ToolEvidence {
    pub kind: &'static str,
    pub summary: &'static str,
}

pub(crate) fn extract(tool: ToolName, bytes: &[u8]) -> [Option<ToolEvidence>; 8] {
    let mut result = [None; 8];
    let Ok(text) = std::str::from_utf8(bytes) else {
        result[0] = Some(ToolEvidence {
            kind: "encoding_invalid",
            summary: "工具输出不是有效 UTF-8，未保留原文",
        });
        return result;
    };
    // 保留首尾各一段用于匹配。未知文本只参与本地判断，不成为报告字段。
    let head: String = text.chars().take(32768).collect();
    let tail: String = text
        .chars()
        .rev()
        .take(32768)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    let lower = format!("{head}\n{tail}").to_ascii_lowercase();
    let apktool = tool == ToolName::Apktool;
    let signing = matches!(tool, ToolName::Apksigner | ToolName::Keytool);
    let rules = [
        (
            apktool && lower.contains("error: resource ") && lower.contains("not found"),
            "resource_not_found",
            "error: resource <资源标识已移除> not found",
        ),
        (
            apktool && lower.contains("error: attribute ") && lower.contains("not found"),
            "attribute_not_found",
            "error: attribute <属性标识已移除> not found",
        ),
        (
            apktool
                && (lower.contains("duplicate value for resource")
                    || lower.contains("duplicate resources")),
            "duplicate_resource",
            "duplicate resource <资源标识已移除>",
        ),
        (
            apktool && (lower.contains("failed to compile") || lower.contains("failed compiling")),
            "resource_compile_failed",
            "资源编译失败（文件路径与资源标识已移除）",
        ),
        (
            apktool && (lower.contains("failed linking") || lower.contains("failed to link")),
            "resource_link_failed",
            "资源链接失败（文件路径与资源标识已移除）",
        ),
        (
            apktool
                && (lower.contains("not well-formed")
                    || lower.contains("xmlpullparserexception")
                    || lower.contains("error parsing xml")),
            "xml_parse_failed",
            "XML 解析失败（文件路径与 XML 内容已移除）",
        ),
        (
            apktool && (lower.contains("not a png file") || lower.contains("png signature")),
            "invalid_png",
            "PNG 格式或签名无效（文件名已移除）",
        ),
        (
            lower.contains("java.lang.unsupportedclassversionerror")
                || lower.contains("unsupported major.minor version"),
            "java_class_version",
            "Java 字节码版本不受当前 JVM 支持",
        ),
        (
            lower.contains("java.lang.outofmemoryerror"),
            "java_out_of_memory",
            "java.lang.OutOfMemoryError（异常详情已移除）",
        ),
        (
            lower.contains("java.util.zip.zipexception"),
            "invalid_zip",
            "java.util.zip.ZipException（异常详情已移除）",
        ),
        (
            lower.contains("permission denied")
                || lower.contains("access is denied")
                || lower.contains("accessdeniedexception"),
            "access_denied",
            "文件访问被拒绝（路径已移除）",
        ),
        (
            lower.contains("no space left on device")
                || lower.contains("not enough space on the disk"),
            "disk_full",
            "磁盘空间不足（路径已移除）",
        ),
        (
            lower.contains("filenotfoundexception")
                || lower.contains("nosuchfileexception")
                || (apktool && lower.contains("was not found or was not readable")),
            "file_not_found",
            "文件不存在或无法打开（路径已移除）",
        ),
        (
            signing
                && (lower.contains("password was incorrect")
                    || lower.contains("password is incorrect")
                    || lower.contains("cannot recover key")),
            "keystore_password",
            "证书口令或私钥解锁失败（口令与证书信息已移除）",
        ),
        (
            signing
                && (lower.contains("no key with alias")
                    || (lower.contains("alias") && lower.contains("does not exist"))),
            "key_alias_missing",
            "证书中未找到指定别名（别名已移除）",
        ),
    ];
    for (slot, (_, kind, summary)) in result
        .iter_mut()
        .zip(rules.into_iter().filter(|rule| rule.0))
    {
        *slot = Some(ToolEvidence { kind, summary });
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 同退出码错误可区分且不携带变量() {
        for (raw, kind) in [
            (
                "/Users/客户/secret.apk: error: resource com.private:string/口令 not found",
                "resource_not_found",
            ),
            (
                "C:\\Users\\客户\\secret.xml: error parsing XML token=PRIVATE",
                "xml_parse_failed",
            ),
            (
                "java.lang.OutOfMemoryError: password=secret",
                "java_out_of_memory",
            ),
        ] {
            let evidence = extract(ToolName::Apktool, raw.as_bytes());
            assert_eq!(evidence[0].unwrap().kind, kind);
            let json = format!("{evidence:?}");
            for secret in ["客户", "secret", "com.private", "PRIVATE", "password="] {
                assert!(!json.contains(secret));
            }
        }
    }

    #[test]
    fn 未知文本和无效编码不透传() {
        assert!(extract(ToolName::Apktool, b"private unknown text")
            .iter()
            .all(Option::is_none));
        assert_eq!(
            extract(ToolName::Apksigner, b"\xff")[0].unwrap().kind,
            "encoding_invalid"
        );
        assert!(
            extract(ToolName::Apksigner, b"error: resource secret not found")
                .iter()
                .all(Option::is_none)
        );
    }

    #[test]
    fn 长输出尾部证据可识别() {
        let raw = format!(
            "{}\njava.util.zip.ZipException: PRIVATE",
            "私密".repeat(40000)
        );
        assert_eq!(
            extract(ToolName::Apktool, raw.as_bytes())[0].unwrap().kind,
            "invalid_zip"
        );
    }
}

use anyhow::{Context, Result};
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NativeLibPackagingPolicy {
    Disabled,
    Enabled,
    Unspecified,
}

pub(crate) fn read_native_lib_packaging_policy(apk_dir: &Path) -> Result<NativeLibPackagingPolicy> {
    let manifest_path = apk_dir.join("AndroidManifest.xml");
    let content = fs::read_to_string(&manifest_path).context("读取 AndroidManifest.xml 失败")?;
    let app_tag_start = content
        .find("<application")
        .context("AndroidManifest.xml 中未找到 <application> 标签")?;
    let app_tag_end = find_tag_end(&content, app_tag_start)
        .context("AndroidManifest.xml <application> 标签未正常关闭")?;
    let app_tag = &content[app_tag_start..app_tag_end];

    Ok(
        match extract_xml_attr(app_tag, "android:extractNativeLibs").as_deref() {
            Some("false") => NativeLibPackagingPolicy::Disabled,
            Some("true") => NativeLibPackagingPolicy::Enabled,
            Some(value) => {
                log::warn!("无法解析 extractNativeLibs 值 {value}，将保留原打包策略");
                NativeLibPackagingPolicy::Unspecified
            }
            None => NativeLibPackagingPolicy::Unspecified,
        },
    )
}

pub(crate) fn modify_manifest(apk_dir: &Path, stub_app: &str) -> Result<()> {
    let manifest_path = apk_dir.join("AndroidManifest.xml");
    let content = fs::read_to_string(&manifest_path).context("读取 AndroidManifest.xml 失败")?;

    let app_tag_start = content
        .find("<application")
        .context("AndroidManifest.xml 中未找到 <application> 标签")?;
    let app_tag_end = find_tag_end(&content, app_tag_start)
        .context("AndroidManifest.xml <application> 标签未正常关闭")?;
    let app_tag = &content[app_tag_start..app_tag_end];

    let orig_app = extract_xml_attr(app_tag, "android:name")
        .unwrap_or_else(|| "android.app.Application".to_string());
    log::info!("检测到原始 Application: {}", orig_app);

    let new_app_tag = set_xml_attr(app_tag, "android:name", stub_app);
    let new_app_tag = remove_xml_attr(&new_app_tag, "android:appComponentFactory");

    let already_injected = content.contains("android:name=\"ORIGINAL_APPLICATION\"");
    let meta_original = if already_injected {
        String::new()
    } else {
        format!(
            "\n        <meta-data\n            android:name=\"ORIGINAL_APPLICATION\"\n            android:value=\"{}\" />",
            orig_app
        )
    };

    let (prefix_tag, suffix) = if new_app_tag.trim_end().ends_with("/>") {
        let tag_body = new_app_tag
            .trim_end()
            .strip_suffix("/>")
            .unwrap_or(&new_app_tag)
            .trim_end_matches(|c: char| c.is_whitespace() || c == '/')
            .to_string();
        (
            format!("{}>", tag_body),
            meta_original + "\n    </application>",
        )
    } else {
        (new_app_tag.to_string(), meta_original)
    };

    let result = format!(
        "{}{}{}{}",
        &content[..app_tag_start],
        prefix_tag,
        suffix,
        &content[app_tag_end..]
    );

    fs::write(&manifest_path, result).context("写入 AndroidManifest.xml 失败")?;
    Ok(())
}

fn find_tag_end(content: &str, start: usize) -> Option<usize> {
    let s = &content[start..];
    let mut in_quote: Option<u8> = None;
    for (i, &b) in s.as_bytes().iter().enumerate() {
        match in_quote {
            Some(q) if b == q => in_quote = None,
            Some(_) => {}
            None => match b {
                b'"' | b'\'' => in_quote = Some(b),
                b'>' => return Some(start + i + 1),
                _ => {}
            },
        }
    }
    None
}

fn extract_xml_attr(tag: &str, attr: &str) -> Option<String> {
    for &q in b"\"'" {
        let needle = format!("{}={}", attr, q as char);
        if let Some(p) = tag.find(&needle) {
            let val_start = p + needle.len();
            let val_end = tag[val_start..].find(q as char)? + val_start;
            return Some(tag[val_start..val_end].to_string());
        }
    }
    None
}

fn set_xml_attr(tag: &str, attr: &str, value: &str) -> String {
    for &q in b"\"'" {
        let needle = format!("{}={}", attr, q as char);
        if let Some(p) = tag.find(&needle) {
            let val_start = p + needle.len();
            let val_end = match tag[val_start..].find(q as char) {
                Some(e) => e + val_start,
                None => return tag.to_string(),
            };
            return format!(
                "{}{}{}\"{}\"{}",
                &tag[..p],
                attr,
                '=',
                value,
                &tag[val_end + 1..]
            );
        }
    }
    let close = find_tag_end(tag, 0).unwrap_or(tag.len());
    format!(
        "{} {}=\"{}\"{}",
        &tag[..close - 1],
        attr,
        value,
        &tag[close - 1..]
    )
}

fn remove_xml_attr(tag: &str, attr: &str) -> String {
    for &q in b"\"'" {
        let needle = format!("{}={}", attr, q as char);
        if let Some(attr_start) = tag.find(&needle) {
            let val_start = attr_start + needle.len();
            let val_end = match tag[val_start..].find(q as char) {
                Some(e) => val_start + e + 1,
                None => continue,
            };
            let rest = tag[val_end..].trim_start_matches(' ');
            let prefix = &tag[..attr_start];
            let prefix = if prefix.ends_with(' ') {
                prefix
            } else {
                prefix.trim_end_matches(' ')
            };
            return format!("{}{}", prefix, rest);
        }
    }
    tag.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn find_tag_end_simple_tag() {
        let s = r#"<application android:label="app">"#;
        assert_eq!(find_tag_end(s, 0), Some(s.len()));
    }

    #[test]
    fn find_tag_end_self_closing() {
        let s = r#"<application />"#;
        assert_eq!(find_tag_end(s, 0), Some(s.len()));
    }

    #[test]
    fn find_tag_end_gt_inside_quoted_attr_skipped() {
        let s = r#"<application android:label="a>b">"#;
        assert_eq!(find_tag_end(s, 0), Some(s.len()));
    }

    #[test]
    fn find_tag_end_with_nonzero_start() {
        let s = "HEADER<application>";
        assert_eq!(find_tag_end(s, 6), Some(s.len()));
    }

    #[test]
    fn extract_xml_attr_double_quoted() {
        let tag = r#"<application android:name="com.example.App">"#;
        assert_eq!(
            extract_xml_attr(tag, "android:name"),
            Some("com.example.App".to_string())
        );
    }

    #[test]
    fn extract_xml_attr_not_found_returns_none() {
        let tag = r#"<application android:label="app">"#;
        assert_eq!(extract_xml_attr(tag, "android:name"), None);
    }

    #[test]
    fn set_xml_attr_replaces_existing_value() {
        let tag = r#"<application android:name="com.example.App">"#;
        let result = set_xml_attr(tag, "android:name", "dev.mocika.StubApp");
        assert!(result.contains(r#"android:name="dev.mocika.StubApp""#));
        assert!(!result.contains("com.example.App"));
    }

    #[test]
    fn set_xml_attr_inserts_when_absent() {
        let tag = r#"<application android:label="app">"#;
        let result = set_xml_attr(tag, "android:name", "dev.mocika.StubApp");
        assert!(result.contains(r#"android:name="dev.mocika.StubApp""#));
    }

    #[test]
    fn remove_xml_attr_removes_found_attr() {
        let tag = r#"<application android:appComponentFactory="abc" android:name="App">"#;
        let result = remove_xml_attr(tag, "android:appComponentFactory");
        assert!(!result.contains("android:appComponentFactory"));
        assert!(result.contains(r#"android:name="App""#));
    }

    #[test]
    fn remove_xml_attr_not_found_returns_original() {
        let tag = r#"<application android:name="App">"#;
        let result = remove_xml_attr(tag, "android:nonExistent");
        assert_eq!(result, tag);
    }

    #[test]
    fn read_native_lib_packaging_policy_distinguishes_three_states() {
        for (value, expected) in [
            (Some("false"), NativeLibPackagingPolicy::Disabled),
            (Some("true"), NativeLibPackagingPolicy::Enabled),
            (None, NativeLibPackagingPolicy::Unspecified),
        ] {
            let dir = tempfile::tempdir().unwrap();
            let attribute = value
                .map(|it| format!(r#" android:extractNativeLibs="{it}""#))
                .unwrap_or_default();
            let manifest =
                format!(r#"<manifest><application{attribute} android:name="App" /></manifest>"#);
            fs::write(dir.path().join("AndroidManifest.xml"), manifest).unwrap();

            assert_eq!(
                read_native_lib_packaging_policy(dir.path()).unwrap(),
                expected
            );
        }
    }

    #[test]
    fn modify_manifest_preserves_extract_native_libs() {
        let dir = tempfile::tempdir().unwrap();
        let manifest = r#"<manifest><application android:name="App" android:extractNativeLibs="false" /></manifest>"#;
        fs::write(dir.path().join("AndroidManifest.xml"), manifest).unwrap();

        modify_manifest(dir.path(), "dev.mocika.StubApp").unwrap();

        let result = fs::read_to_string(dir.path().join("AndroidManifest.xml")).unwrap();
        assert!(result.contains(r#"android:extractNativeLibs="false""#));
    }

    #[test]
    fn modify_manifest_replaces_application_name_and_injects_meta() {
        let dir = tempfile::tempdir().unwrap();
        let manifest = concat!(
            r#"<?xml version="1.0" encoding="utf-8"?>"#,
            "\n<manifest package=\"com.example\">\n",
            "    <application android:name=\"com.example.App\">\n",
            "    </application>\n</manifest>"
        );
        fs::write(dir.path().join("AndroidManifest.xml"), manifest).unwrap();
        modify_manifest(dir.path(), "dev.mocika.StubApp").unwrap();
        let result = fs::read_to_string(dir.path().join("AndroidManifest.xml")).unwrap();
        assert!(result.contains(r#"android:name="dev.mocika.StubApp""#));
        assert!(result.contains("ORIGINAL_APPLICATION"));
        assert!(result.contains("com.example.App"));
    }

    #[test]
    fn modify_manifest_self_closing_application_expanded() {
        let dir = tempfile::tempdir().unwrap();
        let manifest = concat!(
            r#"<?xml version="1.0" encoding="utf-8"?>"#,
            "\n<manifest package=\"com.example\">\n",
            "    <application android:name=\"com.example.App\" />\n",
            "</manifest>"
        );
        fs::write(dir.path().join("AndroidManifest.xml"), manifest).unwrap();
        modify_manifest(dir.path(), "dev.mocika.StubApp").unwrap();
        let result = fs::read_to_string(dir.path().join("AndroidManifest.xml")).unwrap();
        assert!(result.contains(r#"android:name="dev.mocika.StubApp""#));
        assert!(result.contains("</application>"));
        assert!(result.contains("ORIGINAL_APPLICATION"));
    }

    #[test]
    fn modify_manifest_already_injected_no_duplicate_meta() {
        let dir = tempfile::tempdir().unwrap();
        let manifest = concat!(
            r#"<?xml version="1.0" encoding="utf-8"?>"#,
            "\n<manifest package=\"com.example\">\n",
            "    <application android:name=\"dev.mocika.StubApp\">\n",
            "        <meta-data android:name=\"ORIGINAL_APPLICATION\"",
            " android:value=\"com.example.App\" />\n",
            "    </application>\n</manifest>"
        );
        fs::write(dir.path().join("AndroidManifest.xml"), manifest).unwrap();
        modify_manifest(dir.path(), "dev.mocika.StubApp").unwrap();
        let result = fs::read_to_string(dir.path().join("AndroidManifest.xml")).unwrap();
        assert_eq!(result.matches("ORIGINAL_APPLICATION").count(), 1);
    }
}

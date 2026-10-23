//! 只读检查 Application 直属元数据，避免覆盖原包的运行字段。

use anyhow::{Context, Result};

pub(super) fn ensure_available(content: &str, reserved: &[&str]) -> Result<()> {
    let mut cursor = 0;
    let mut stack = Vec::new();
    while let Some(offset) = content[cursor..].find('<') {
        let start = cursor + offset;
        let remaining = &content[start..];
        let special = if remaining.starts_with("<!--") {
            Some("-->")
        } else if remaining.starts_with("<![CDATA[") {
            Some("]]>")
        } else if remaining.starts_with("<?") {
            Some("?>")
        } else {
            None
        };
        if let Some(end) = special {
            cursor = start + remaining.find(end).context("Manifest 注释或声明未结束")? + end.len();
            continue;
        }
        let mut quote = None;
        let end = remaining
            .char_indices()
            .find_map(|(index, value)| match (quote, value) {
                (Some(previous), current) if previous == current => {
                    quote = None;
                    None
                }
                (Some(_), _) => None,
                (None, '\'' | '"') => {
                    quote = Some(value);
                    None
                }
                (None, '>') => Some(index),
                _ => None,
            })
            .context("Manifest 标签未正常关闭")?;
        cursor = start + end + 1;
        let body = remaining[1..end].trim();
        if let Some(name) = body.strip_prefix('/') {
            if stack.pop() != Some(name.trim()) {
                anyhow::bail!("Manifest 标签嵌套无效");
            }
            continue;
        }
        let name_end = body
            .find(|value: char| value.is_whitespace() || value == '/')
            .unwrap_or(body.len());
        let name = &body[..name_end];
        if name.is_empty() || name.starts_with('!') {
            anyhow::bail!("Manifest 包含不支持的标签声明");
        }
        if name == "meta-data" && stack.as_slice() == ["manifest", "application"] {
            if let Some(value) = attribute(&body[name_end..], "android:name")? {
                let decoded = decode_name(value)?;
                if reserved.contains(&decoded.as_str()) {
                    anyhow::bail!("原 APK 已占用运行资源保留字段 {decoded}，无法安全加固");
                }
            }
        }
        if !body.ends_with('/') {
            stack.push(name);
        }
    }
    if !stack.is_empty() {
        anyhow::bail!("Manifest 标签未结束");
    }
    Ok(())
}

fn attribute<'a>(mut body: &'a str, wanted: &str) -> Result<Option<&'a str>> {
    let mut found = None;
    loop {
        body = body.trim_start();
        if body.is_empty() || body == "/" {
            return Ok(found);
        }
        let end = body
            .find(|value: char| value.is_whitespace() || value == '=')
            .context("Manifest 属性无效")?;
        let name = &body[..end];
        body = body[end..]
            .trim_start()
            .strip_prefix('=')
            .context("Manifest 属性缺少等号")?
            .trim_start();
        let quote = body
            .chars()
            .next()
            .filter(|value| matches!(value, '\'' | '"'))
            .context("Manifest 属性缺少引号")?;
        body = &body[1..];
        let end = body.find(quote).context("Manifest 属性引号未结束")?;
        if name == wanted {
            if found.is_some() {
                anyhow::bail!("Manifest 元数据名称属性重复");
            }
            found = Some(&body[..end]);
        }
        body = &body[end + 1..];
    }
}

fn decode_name(mut value: &str) -> Result<String> {
    let mut decoded = String::new();
    while let Some(start) = value.find('&') {
        decoded.push_str(&value[..start]);
        let end = value[start..]
            .find(';')
            .context("Manifest 字段实体未结束")?
            + start;
        let entity = &value[start + 1..end];
        let character = match entity {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            _ => entity
                .strip_prefix("#x")
                .and_then(|text| u32::from_str_radix(text, 16).ok())
                .or_else(|| entity.strip_prefix('#').and_then(|text| text.parse().ok()))
                .and_then(char::from_u32),
        }
        .context("Manifest 字段实体无效")?;
        decoded.push(character);
        value = &value[end + 1..];
    }
    decoded.push_str(value);
    Ok(decoded)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 仅检查直属元数据而不误报业务内容() {
        let content = r#"<manifest><application label="rt.app"><!-- <meta-data android:name="rt.app"/> -->
            <activity><meta-data android:name="rt.app"/></activity>
            <meta-data android:name="other" android:value="rt.app"/>
            </application></manifest>"#;
        ensure_available(content, &["rt.app"]).unwrap();
    }

    #[test]
    fn 保留字段的引号空白实体及重复均不能绕过() {
        for name in ["rt.app", "rt.&#97;pp", "rt.&#x61;pp"] {
            let content = format!("<manifest><application><meta-data android:name = '{name}' /></application></manifest>");
            assert!(ensure_available(&content, &["rt.app"])
                .unwrap_err()
                .to_string()
                .contains("已占用"));
        }
        assert!(ensure_available("<manifest><application><meta-data android:name='other' android:name='rt.app'/></application></manifest>", &["rt.app"]).is_err());
    }

    #[test]
    fn 畸形标签与实体明确失败() {
        for content in ["<manifest><application>", "<manifest><application></manifest>", "<manifest><!--", "<manifest><application><meta-data android:name='rt.&unknown;'/></application></manifest>"] {
            assert!(ensure_available(content, &["rt.app"]).is_err());
        }
    }
}

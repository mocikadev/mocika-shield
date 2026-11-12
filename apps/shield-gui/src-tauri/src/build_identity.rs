//! 只描述当前构建，不读取进程、路径或用户资料；名称匹配不认证发行来源。
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct BuildIdentity {
    pub product_name: String,
    pub application_id: String,
    pub build_revision: Option<String>,
}

impl BuildIdentity {
    pub(crate) fn from_build(
        product_name: Option<&str>,
        application_id: &str,
        revision: Option<&str>,
    ) -> Option<Self> {
        let value = Self {
            product_name: product_name?.to_owned(),
            application_id: application_id.to_owned(),
            build_revision: revision
                .filter(|value| valid_revision(value))
                .map(str::to_owned),
        };
        value.is_valid().then_some(value)
    }

    pub(crate) fn is_valid(&self) -> bool {
        let name = &self.product_name;
        !name.is_empty()
            && name.len() <= 128
            && name.trim() == name
            && !name.chars().any(char::is_control)
            && !name.contains(['/', '\\', ':', '*', '?', '"', '<', '>', '|'])
            && !self.application_id.is_empty()
            && self.application_id.len() <= 128
            && self
                .application_id
                .bytes()
                .all(|value| value.is_ascii_alphanumeric() || matches!(value, b'.' | b'-'))
            && self.build_revision.as_deref().is_none_or(valid_revision)
    }

    pub(crate) fn key(&self) -> String {
        let tuple = [
            Some(self.product_name.as_str()),
            Some(self.application_id.as_str()),
            self.build_revision.as_deref(),
        ];
        let bytes = serde_json::to_vec(&tuple).expect("构建身份字符串可序列化");
        format!("{:x}", Sha256::digest(bytes))
    }
}

fn valid_revision(value: &str) -> bool {
    (7..=40).contains(&value.len())
        && value
            .bytes()
            .all(|value| value.is_ascii_digit() || (b'a'..=b'f').contains(&value))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 构建改名和同版不同提交自动形成不同键() {
        let original =
            BuildIdentity::from_build(Some("测试工具"), "dev.example.tool", Some("abcdef01"))
                .unwrap();
        let renamed =
            BuildIdentity::from_build(Some("另一个工具"), "dev.example.tool", Some("abcdef01"))
                .unwrap();
        let rebuilt =
            BuildIdentity::from_build(Some("测试工具"), "dev.example.tool", Some("abcdef02"))
                .unwrap();
        assert_ne!(original.key(), renamed.key());
        assert_ne!(original.key(), rebuilt.key());
        assert_eq!(original.key(), original.clone().key());
    }

    #[test]
    fn 中文及补充字符来源键与服务端测试向量一致() {
        let source =
            BuildIdentity::from_build(Some("测试工具😀"), "dev.example.tool", Some("abcdef01"))
                .unwrap();
        assert_eq!(
            source.key(),
            "e367c5a1d518162d748d321e80841f443605bb2b018a73a2671e61f2fe9eca15"
        );
    }

    #[test]
    fn 缺身份不伪造项目名且非法提交号只留空() {
        assert!(BuildIdentity::from_build(None, "dev.example.tool", None).is_none());
        for name in ["", " 名称", "名称\n", "/本机/路径", "不合法:名称"] {
            assert!(BuildIdentity::from_build(Some(name), "dev.example.tool", None).is_none());
        }
        assert!(BuildIdentity::from_build(Some("工具"), "bad/id", None).is_none());
        assert!(
            BuildIdentity::from_build(Some(&"中".repeat(43)), "dev.example.tool", None).is_none()
        );
        let source =
            BuildIdentity::from_build(Some("工具😀"), "dev.example.tool", Some("unknown")).unwrap();
        assert!(source.build_revision.is_none());
        assert!(source.is_valid());
    }
}

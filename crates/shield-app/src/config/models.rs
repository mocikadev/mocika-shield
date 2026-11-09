//! 与界面无关的加固偏好及字段更新语义；未指定、显式清空和设置值分开处理。
use super::ConfigError;
use serde::{Deserialize, Deserializer, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct ProtectDefaults {
    pub runtime_mode: String,
    pub environment_policy: String,
    pub sign_after_protect: Option<bool>,
    pub certificate_id: Option<String>,
    pub output_directory_mode: String,
    pub fixed_output_directory: String,
}
impl Default for ProtectDefaults {
    fn default() -> Self {
        Self {
            runtime_mode: "standard".into(),
            environment_policy: "compatible".into(),
            sign_after_protect: None,
            certificate_id: None,
            output_directory_mode: "source".into(),
            fixed_output_directory: String::new(),
        }
    }
}
pub fn normalize_protect_defaults(value: &ProtectDefaults) -> ProtectDefaults {
    ProtectDefaults {
        runtime_mode: if value.runtime_mode == "android_api19" {
            "android_api19"
        } else {
            "standard"
        }
        .into(),
        environment_policy: if value.environment_policy == "strict" {
            "strict"
        } else {
            "compatible"
        }
        .into(),
        sign_after_protect: value.sign_after_protect,
        certificate_id: value
            .certificate_id
            .as_deref()
            .map(str::trim)
            .filter(|id| !id.is_empty())
            .map(str::to_owned),
        output_directory_mode: if value.output_directory_mode == "fixed" {
            "fixed"
        } else {
            "source"
        }
        .into(),
        fixed_output_directory: value.fixed_output_directory.trim().to_owned(),
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Setting<T> {
    #[default]
    Keep,
    Set(T),
}
impl<'de, T: Deserialize<'de>> Deserialize<'de> for Setting<T> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        T::deserialize(deserializer).map(Self::Set)
    }
}
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct ProtectDefaultsPatch {
    pub runtime_mode: Setting<String>,
    pub environment_policy: Setting<String>,
    pub sign_after_protect: Setting<Option<bool>>,
    pub certificate_id: Setting<Option<String>>,
    pub output_directory_mode: Setting<String>,
    pub fixed_output_directory: Setting<String>,
}
impl ProtectDefaultsPatch {
    pub fn apply(self, target: &mut ProtectDefaults) -> Result<(), ConfigError> {
        if matches!(&self.runtime_mode, Setting::Set(value) if !["standard","android_api19"].contains(&value.as_str()))
            || matches!(&self.environment_policy, Setting::Set(value) if !["compatible","strict"].contains(&value.as_str()))
            || matches!(&self.output_directory_mode, Setting::Set(value) if !["source","fixed"].contains(&value.as_str()))
        {
            return Err(ConfigError::InvalidValue);
        }
        if let Setting::Set(value) = self.runtime_mode {
            target.runtime_mode = value;
        }
        if let Setting::Set(value) = self.environment_policy {
            target.environment_policy = value;
        }
        if let Setting::Set(value) = self.sign_after_protect {
            target.sign_after_protect = value;
        }
        if let Setting::Set(value) = self.certificate_id {
            target.certificate_id = value;
        }
        if let Setting::Set(value) = self.output_directory_mode {
            target.output_directory_mode = value;
        }
        if let Setting::Set(value) = self.fixed_output_directory {
            target.fixed_output_directory = value;
        }
        *target = normalize_protect_defaults(target);
        Ok(())
    }
}

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub(super) struct BusinessConfig {
    pub protect_defaults: ProtectDefaults,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn 未指定保持原值而空值明确清除() {
        let mut target = ProtectDefaults {
            certificate_id: Some("cert".into()),
            sign_after_protect: Some(true),
            ..Default::default()
        };
        serde_json::from_str::<ProtectDefaultsPatch>(r#"{"environment_policy":"strict"}"#)
            .unwrap()
            .apply(&mut target)
            .unwrap();
        assert_eq!(target.certificate_id.as_deref(), Some("cert"));
        serde_json::from_str::<ProtectDefaultsPatch>(
            r#"{"certificate_id":null,"sign_after_protect":null}"#,
        )
        .unwrap()
        .apply(&mut target)
        .unwrap();
        assert_eq!(target.certificate_id, None);
        assert_eq!(target.sign_after_protect, None);
        assert!(serde_json::from_str::<ProtectDefaultsPatch>(r#"{"runtime_mode":null}"#).is_err());
    }
}

//! 加固计划的来源快照与固定材料集合；不解析密码、执行工具或提交产物。
use crate::automation::{
    snapshot::{self, FileSnapshot},
    CheckError,
};
use serde::Serialize;

#[derive(Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::automation) struct Sources {
    pub config: Option<FileSnapshot>,
    #[cfg(all(
        feature = "agent-executor",
        any(target_os = "macos", target_os = "linux", windows)
    ))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shared: Option<crate::automation::shared_plan::SharedIdentity>,
    pub input: FileSnapshot,
    pub keystore: FileSnapshot,
    pub java: FileSnapshot,
    pub keytool: FileSnapshot,
    pub apktool: FileSnapshot,
    pub apksigner: FileSnapshot,
    pub resources: FileSnapshot,
    #[cfg(all(
        feature = "agent-executor",
        any(target_os = "macos", target_os = "linux", windows)
    ))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub java_adapter: Option<crate::automation::adapter::Binding>,
}

impl Sources {
    pub fn recheck(&self) -> Result<(), CheckError> {
        snapshot::recheck(&self.files())?;
        #[cfg(all(
            feature = "agent-executor",
            any(target_os = "macos", target_os = "linux", windows)
        ))]
        if let Some(binding) = &self.java_adapter {
            binding.recheck(&mut || Ok(()))?;
        }
        #[cfg(all(
            feature = "agent-executor",
            any(target_os = "macos", target_os = "linux", windows)
        ))]
        if let Some(shared) = &self.shared {
            shared.recheck()?;
        }
        Ok(())
    }
    pub fn files(&self) -> Vec<&FileSnapshot> {
        let files = self
            .config
            .iter()
            .chain([
                &self.input,
                &self.keystore,
                &self.java,
                &self.keytool,
                &self.apktool,
                &self.apksigner,
                &self.resources,
            ])
            .collect::<Vec<_>>();
        #[cfg(all(
            feature = "agent-executor",
            any(target_os = "macos", target_os = "linux", windows)
        ))]
        let files = files
            .into_iter()
            .chain(self.java_adapter.iter().map(|b| b.snapshot()))
            .collect();
        files
    }
    #[cfg(all(
        feature = "agent-executor",
        any(target_os = "macos", target_os = "linux", windows)
    ))]
    pub fn materials(&self) -> Vec<(crate::automation::task_materials::Role, &FileSnapshot)> {
        use crate::automation::task_materials::Role;
        let mut values = vec![(Role::Input, &self.input)];
        if let Some(config) = &self.config {
            values.push((Role::Config, config));
        }
        values.extend([
            (Role::Keystore, &self.keystore),
            (Role::Apktool, &self.apktool),
            (Role::Apksigner, &self.apksigner),
            (Role::Resources, &self.resources),
        ]);
        values.extend(
            self.java_adapter
                .iter()
                .map(|b| (Role::JavaAdapter, b.snapshot())),
        );
        values
    }
    #[cfg(all(
        feature = "agent-executor",
        any(target_os = "macos", target_os = "linux", windows)
    ))]
    pub fn material_version(&self) -> u32 {
        if self.java_adapter.is_some() {
            4
        } else if self.shared.is_some() {
            2
        } else {
            1
        }
    }
}

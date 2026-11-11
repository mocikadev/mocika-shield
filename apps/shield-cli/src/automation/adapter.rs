//! 正式适配资源的选择、身份和私有副本绑定；不拥有工作根、业务规则或提交权。
use super::{
    executable::{regular_stamp, Stamp},
    snapshot::{self, FileSnapshot},
    CheckError,
};
use serde::{Deserialize, Serialize};
use shield_core::JavaToolContext;
use std::path::Path;

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(super) struct Binding {
    file: FileSnapshot,
    stamp: Stamp,
}
impl Binding {
    pub fn capture(path: &Path) -> Result<Self, CheckError> {
        let resolved = super::task_files::resolved(path).map_err(|_| invalid())?;
        if std::fs::metadata(&resolved).map_err(|_| invalid())?.len() > 256 * 1024 {
            return Err(invalid());
        }
        let before = regular_stamp(&resolved).map_err(|_| invalid())?;
        let file = snapshot::file("java_adapter", path).map_err(|_| invalid())?;
        let stamp = regular_stamp(&file.path).map_err(|_| invalid())?;
        if before != stamp
            || file.path != resolved
            || super::task_files::resolved(&file.lookup_path).ok().as_ref() != Some(&resolved)
        {
            return Err(invalid());
        }
        Ok(Self { file, stamp })
    }
    pub fn snapshot(&self) -> &FileSnapshot {
        &self.file
    }
    pub fn recheck(
        &self,
        check: &mut impl FnMut() -> Result<(), CheckError>,
    ) -> Result<(), CheckError> {
        check()?;
        let after = Self::capture(&self.file.lookup_path).map_err(|_| invalid())?;
        if self != &after {
            return Err(invalid());
        }
        check()
    }
}

pub(super) fn invalid() -> CheckError {
    CheckError::new(
        "TOOLCHAIN_CHANGED",
        "Java 适配材料缺失、不兼容或已变化；请重新构建并规划，不会回退旧参数。",
    )
}

pub(super) fn require(binding: Option<&Binding>) -> Result<(), CheckError> {
    if cfg!(windows) != binding.is_some()
        || binding.is_some_and(|b| b.snapshot().role != "java_adapter")
    {
        return Err(invalid());
    }
    Ok(())
}

#[cfg(windows)]
pub(super) fn select() -> Result<Binding, CheckError> {
    let path = shield_core::find_java_adapter().map_err(|_| invalid())?;
    Binding::capture(&path)
}

#[derive(Clone)]
pub(super) struct Guard {
    source: Binding,
    selected: Binding,
}
impl Guard {
    pub fn recheck(&self) -> Result<(), CheckError> {
        self.source.recheck(&mut || Ok(()))?;
        self.selected.recheck(&mut || Ok(()))
    }
}

pub(super) struct Bound {
    pub context: JavaToolContext,
    guard: Guard,
}
impl Bound {
    pub fn new(binding: &Binding, selected: &Path, workspace: &Path) -> Result<Self, CheckError> {
        if binding.snapshot().role != "java_adapter" {
            return Err(invalid());
        }
        binding.recheck(&mut || Ok(()))?;
        let selected = Binding::capture(selected)?;
        let before: &FileSnapshot = binding.snapshot();
        if selected.snapshot().size != before.size || selected.snapshot().sha256 != before.sha256 {
            return Err(invalid());
        }
        let context =
            JavaToolContext::new(&selected.snapshot().path, workspace).map_err(|_| invalid())?;
        let digest: String = context
            .resource_sha256()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        if digest != before.sha256 {
            return Err(invalid());
        }
        let bound = Self {
            context,
            guard: Guard {
                source: binding.clone(),
                selected,
            },
        };
        bound.guard.recheck()?;
        Ok(bound)
    }
    pub fn guard(&self) -> Guard {
        self.guard.clone()
    }
    pub fn recheck(&self) -> Result<(), CheckError> {
        self.guard.recheck()
    }
}

#[cfg(test)]
#[path = "adapter_tests.rs"]
mod tests;

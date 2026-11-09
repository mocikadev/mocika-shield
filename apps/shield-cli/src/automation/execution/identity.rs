//! 可执行文件身份：发现持久替换/改向，不冻结完整工具链，不提供同权限攻击隔离。
use super::*;
use crate::automation::{
    plan::Sources,
    snapshot::{self, FileSnapshot},
};

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct Stamp {
    device: u64,
    inode: u64,
    mode: u32,
    bytes: u64,
    modified: i64,
    modified_nanos: i64,
    changed: i64,
    changed_nanos: i64,
}
impl Stamp {
    fn read(path: &Path) -> Result<Self, CheckError> {
        let metadata = std::fs::metadata(path).map_err(|_| error("EXECUTABLE_CHANGED"))?;
        if !metadata.is_file() || metadata.mode() & 0o111 == 0 {
            return Err(error("EXECUTABLE_CHANGED"));
        }
        Ok(Self {
            device: metadata.dev(),
            inode: metadata.ino(),
            mode: metadata.mode(),
            bytes: metadata.len(),
            modified: metadata.mtime(),
            modified_nanos: metadata.mtime_nsec(),
            changed: metadata.ctime(),
            changed_nanos: metadata.ctime_nsec(),
        })
    }
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(super) struct Executable {
    file: FileSnapshot,
    stamp: Stamp,
}
impl Executable {
    pub fn capture(
        role: &str,
        path: &Path,
        check: &mut impl FnMut() -> Result<(), CheckError>,
    ) -> Result<Self, CheckError> {
        check()?;
        let resolved = std::fs::canonicalize(path).map_err(|_| error("EXECUTABLE_CHANGED"))?;
        let before = Stamp::read(&resolved)?;
        let file = snapshot::file_checked(role, path, check)?;
        let stamp = Stamp::read(&file.path)?;
        if file.path != resolved
            || before != stamp
            || file.size != stamp.bytes
            || std::fs::canonicalize(&file.lookup_path).ok().as_ref() != Some(&resolved)
        {
            return Err(error("EXECUTABLE_CHANGED"));
        }
        Ok(Self { file, stamp })
    }

    pub fn recheck(
        &self,
        check: &mut impl FnMut() -> Result<(), CheckError>,
    ) -> Result<(), CheckError> {
        let after = Self::capture(&self.file.role, &self.file.lookup_path, check)?;
        if self != &after {
            return Err(error("EXECUTABLE_CHANGED"));
        }
        Ok(())
    }

    pub fn quick(
        &self,
        check: &mut impl FnMut() -> Result<(), CheckError>,
    ) -> Result<(), CheckError> {
        check()?;
        if std::fs::canonicalize(&self.file.lookup_path).ok().as_ref() != Some(&self.file.path)
            || Stamp::read(&self.file.path)? != self.stamp
        {
            return Err(error("EXECUTABLE_CHANGED"));
        }
        check()
    }

    pub fn selected(
        &self,
        program: &Path,
        check: &mut impl FnMut() -> Result<(), CheckError>,
    ) -> Result<(), CheckError> {
        if program != self.file.lookup_path {
            return Err(error("EXECUTABLE_CHANGED"));
        }
        self.quick(check)
    }

    pub fn worker(&self) -> Result<(), CheckError> {
        if self.file.role != "executor" {
            return Err(error("EXECUTABLE_CHANGED"));
        }
        self.quick(&mut || Ok(()))?;
        let current = std::env::current_exe()
            .and_then(std::fs::canonicalize)
            .map_err(|_| error("EXECUTABLE_CHANGED"))?;
        // 从链接启动时 current_exe 的查找路径可以不同，但实际文件及摘要必须相同。
        if current != self.file.path || Stamp::read(&current)? != self.stamp {
            return Err(error("EXECUTABLE_CHANGED"));
        }
        Ok(())
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Tools {
    java: Executable,
    keytool: Executable,
}
impl Tools {
    fn changed(err: CheckError) -> CheckError {
        if matches!(err.code, "TASK_CANCELLED" | "TASK_TIMEOUT") {
            err
        } else {
            error("TOOLCHAIN_CHANGED")
        }
    }
    pub fn capture(
        sources: &Sources,
        check: &mut impl FnMut() -> Result<(), CheckError>,
    ) -> Result<Self, CheckError> {
        Self::capture_files(&sources.java, &sources.keytool, check)
    }
    pub fn capture_files(
        java: &FileSnapshot,
        keytool: &FileSnapshot,
        check: &mut impl FnMut() -> Result<(), CheckError>,
    ) -> Result<Self, CheckError> {
        let tools = Self {
            java: Executable::capture("java", &java.lookup_path, check).map_err(Self::changed)?,
            keytool: Executable::capture("keytool", &keytool.lookup_path, check)
                .map_err(Self::changed)?,
        };
        tools.matches_files(java, keytool)?;
        Ok(tools)
    }

    pub fn matches(&self, sources: &Sources) -> Result<(), CheckError> {
        self.matches_files(&sources.java, &sources.keytool)
    }
    pub fn matches_files(
        &self,
        java: &FileSnapshot,
        keytool: &FileSnapshot,
    ) -> Result<(), CheckError> {
        if self.java.file != *java || self.keytool.file != *keytool {
            return Err(error("TOOLCHAIN_CHANGED"));
        }
        Ok(())
    }

    pub fn recheck(
        &self,
        check: &mut impl FnMut() -> Result<(), CheckError>,
    ) -> Result<(), CheckError> {
        self.java
            .recheck(check)
            .and_then(|_| self.keytool.recheck(check))
            .map_err(Self::changed)
    }

    pub fn pin(&self, command: &mut std::process::Command) -> Result<(), CheckError> {
        let inherited = command
            .get_envs()
            .find(|(key, _)| *key == "PATH")
            .map(|(_, value)| value.map(std::ffi::OsStr::to_os_string))
            .unwrap_or_else(|| std::env::var_os("PATH"));
        let mut paths = vec![self
            .java
            .file
            .lookup_path
            .parent()
            .ok_or_else(|| error("TOOLCHAIN_CHANGED"))?
            .to_owned()];
        if let Some(path) = inherited {
            paths.extend(std::env::split_paths(&path));
        }
        command.env(
            "PATH",
            std::env::join_paths(paths).map_err(|_| error("TOOLCHAIN_CHANGED"))?,
        );
        Ok(())
    }
}

#[cfg(test)]
#[path = "identity_tests.rs"]
mod tests;

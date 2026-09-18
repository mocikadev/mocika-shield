//! 有界检查引用与任务快照；身份复核不改变原任务的成功或失败。
use super::{
    protocol::{utc_date, Submission},
    SharingChoice, SharingState,
};
use crate::{
    app_config::AppConfigState,
    application_identity::{read_application_identity, ApplicationIdentity},
    task_manager::{TaskKind, TaskStatus},
};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    time::SystemTime,
};

#[derive(Clone, PartialEq, Eq)]
pub(super) struct FileIdentity {
    canonical: PathBuf,
    length: u64,
    modified: SystemTime,
    #[cfg(unix)]
    inode: (u64, u64),
}

impl FileIdentity {
    pub(super) fn read(path: &Path) -> Option<Self> {
        let metadata = std::fs::metadata(path).ok()?;
        if !metadata.is_file() {
            return None;
        }
        #[cfg(unix)]
        use std::os::unix::fs::MetadataExt;
        Some(Self {
            canonical: std::fs::canonicalize(path).ok()?,
            length: metadata.len(),
            modified: metadata.modified().ok()?,
            #[cfg(unix)]
            inode: (metadata.dev(), metadata.ino()),
        })
    }
}

struct Inspection {
    path: PathBuf,
    file: FileIdentity,
    identity: ApplicationIdentity,
    created: u64,
}

#[derive(Default)]
pub(super) struct InspectionStore {
    references: HashMap<String, Inspection>,
    pub(super) tasks: HashMap<String, Frozen>,
}

impl InspectionStore {
    pub(super) fn read_input(path: &Path) -> Option<(FileIdentity, ApplicationIdentity)> {
        let before = FileIdentity::read(path)?;
        let identity = read_application_identity(path).ok()?;
        if Some(&before) != FileIdentity::read(path).as_ref() {
            return None;
        }
        Some((before, identity))
    }

    pub(super) fn insert(
        &mut self,
        path: PathBuf,
        file: FileIdentity,
        identity: ApplicationIdentity,
        now: u64,
        mut on_evict: impl FnMut(&str),
    ) -> String {
        self.references.retain(|_, entry| {
            if now.saturating_sub(entry.created) < 300 {
                return true;
            }
            // 丢失最后的可信撤销绑定前，必须先让旧任务与重试失败关闭。
            on_evict(&entry.identity.package_name);
            false
        });
        if self.references.len() >= 64 {
            if let Some(oldest) = self
                .references
                .iter()
                .min_by_key(|(_, entry)| entry.created)
                .map(|(id, _)| id.clone())
            {
                on_evict(&self.references[&oldest].identity.package_name);
                self.references.remove(&oldest);
            }
        }
        let id = uuid::Uuid::new_v4().to_string();
        self.references.insert(
            id.clone(),
            Inspection {
                path,
                file,
                identity,
                created: now,
            },
        );
        id
    }

    pub(super) fn release(&mut self, id: &str) {
        self.references.remove(id);
    }

    pub(super) fn package(&self, id: &str, now: u64) -> Option<&str> {
        let entry = self.references.get(id)?;
        (now.saturating_sub(entry.created) < 300).then_some(entry.identity.package_name.as_str())
    }

    pub(super) fn preference_package(
        &mut self,
        id: &str,
        now: u64,
        enabled: bool,
    ) -> Option<String> {
        let entry = self.references.get_mut(id)?;
        if FileIdentity::read(&entry.path).as_ref() != Some(&entry.file) {
            // 已知引用的退出始终作用于原包名，不能因输入移动而留下旧队列。
            return (!enabled).then(|| entry.identity.package_name.clone());
        }
        // 偏好操作在同一文件身份下续期；任务启动仍拒绝过期引用。
        entry.created = now;
        Some(entry.identity.package_name.clone())
    }

    pub(super) fn freeze(
        &self,
        state: &SharingState,
        config: &AppConfigState,
        path: &Path,
        choice: Option<&SharingChoice>,
        flow: &str,
        now: u64,
    ) -> Option<Frozen> {
        let choice = choice.filter(|choice| choice.enabled)?;
        let reference = self.references.get(&choice.inspection_id)?;
        if now.saturating_sub(reference.created) >= 300
            || reference.path != path
            || FileIdentity::read(path).as_ref() != Some(&reference.file)
        {
            return None;
        }
        let generation = state.authorization(config, &reference.identity.package_name)?;
        Some(Frozen {
            identity: reference.identity.clone(),
            generation,
            file: Some(reference.file.clone()),
            path: path.to_path_buf(),
            valid: true,
            flow: flow.into(),
            version: env!("CARGO_PKG_VERSION").into(),
            protect_date: None,
        })
    }
}

pub(super) struct Frozen {
    identity: ApplicationIdentity,
    pub(super) generation: u64,
    file: Option<FileIdentity>,
    path: PathBuf,
    valid: bool,
    flow: String,
    version: String,
    protect_date: Option<String>,
}

impl Frozen {
    pub(super) fn verify_input(&mut self) {
        self.valid &= self.file.is_some() && FileIdentity::read(&self.path) == self.file;
    }

    pub(super) fn protected(&mut self, now: u64) {
        self.protect_date.get_or_insert_with(|| utc_date(now));
    }

    pub(super) fn submission(
        self,
        kind: TaskKind,
        status: TaskStatus,
        protected: bool,
        now: u64,
    ) -> Option<Submission> {
        if !self.valid || status == TaskStatus::Cancelled {
            return None;
        }
        let date = match kind {
            TaskKind::Protect if protected && self.flow != "sign" => self.protect_date?,
            TaskKind::Sign if status == TaskStatus::Succeeded && self.flow == "sign" => {
                utc_date(now)
            }
            _ => return None,
        };
        Some(Submission::new(
            self.identity,
            &self.flow,
            date,
            self.version,
        ))
    }

    #[cfg(test)]
    pub(super) fn test_snapshot(identity: ApplicationIdentity, flow: &str) -> Self {
        Self {
            identity,
            generation: 0,
            file: None,
            path: PathBuf::new(),
            valid: true,
            flow: flow.into(),
            version: "1.4.0-beta.5".into(),
            protect_date: None,
        }
    }
}

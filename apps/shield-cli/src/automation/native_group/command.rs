//! 普通 CLI 的完整启动描述；不保存凭据，不接受 shell 文本或继承隐式 Command 状态。
use std::{
    ffi::{OsStr, OsString},
    path::{Path, PathBuf},
};

pub(crate) struct CommandSpec {
    pub(super) program: PathBuf,
    pub(super) arguments: Vec<OsString>,
    pub(super) directory: PathBuf,
    pub(super) environment: Vec<(OsString, OsString)>,
}

impl CommandSpec {
    pub fn new(program: &Path, arguments: Vec<OsString>, directory: &Path) -> Self {
        Self {
            program: program.to_owned(),
            arguments,
            directory: directory.to_owned(),
            environment: std::env::vars_os().collect(),
        }
    }

    pub fn remove_env(&mut self, name: &str) {
        self.environment
            .retain(|(key, _)| !same_key(key, OsStr::new(name)));
    }

    #[cfg(test)]
    pub fn set_env(&mut self, name: &str, value: &OsStr) {
        self.remove_env(name);
        self.environment.push((name.into(), value.to_owned()));
    }

    #[cfg(unix)]
    pub(super) fn into_command(self) -> std::process::Command {
        let mut command = shield_core::utils::no_window_command(self.program);
        command
            .args(self.arguments)
            .current_dir(self.directory)
            .env_clear()
            .envs(self.environment);
        command
    }
}

fn same_key(first: &OsStr, second: &OsStr) -> bool {
    #[cfg(windows)]
    {
        first.eq_ignore_ascii_case(second)
    }
    #[cfg(unix)]
    {
        first == second
    }
}

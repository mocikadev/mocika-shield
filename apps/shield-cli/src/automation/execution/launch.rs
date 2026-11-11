//! 同一执行器的启动与局部工具选择；不管理任务终态或最终产物。
use super::{
    channel::Pump,
    identity::{Executable, Tools},
    *,
};
use crate::automation::native_group::ManagedGroup;
use std::process::Command;

pub(super) struct Launcher<F> {
    command: F,
    executor: Option<Executable>,
}

// 仅表达启动边界的资源责任，不另建业务任务状态机。
pub(super) enum LaunchError {
    /// 没有活动工作端：尚未启动，或已确认停止，可由拥有者显式清理暂存。
    Inactive(CheckError),
    /// 启动或停止无法确认，拥有者必须保留目录，不能靠析构宣称清理完成。
    Unconfirmed,
}

fn changed(err: CheckError) -> CheckError {
    if matches!(err.code, "TASK_TIMEOUT" | "TASK_CANCELLED") {
        err
    } else {
        error("EXECUTOR_CHANGED")
    }
}

impl<F: FnMut() -> Command> Launcher<F> {
    pub fn new(command: F) -> Self {
        Self {
            command,
            executor: None,
        }
    }

    pub fn recheck(&self, deadline: Instant, cancel: &AtomicBool) -> Result<(), CheckError> {
        self.executor
            .as_ref()
            .ok_or_else(|| error("EXECUTOR_CHANGED"))?
            .quick(&mut || checkpoint(deadline, cancel))
            .map_err(changed)
    }

    pub fn verify(&self, deadline: Instant, cancel: &AtomicBool) -> Result<(), CheckError> {
        self.executor
            .as_ref()
            .ok_or_else(|| error("EXECUTOR_CHANGED"))?
            .recheck(&mut || checkpoint(deadline, cancel))
            .map_err(changed)
    }

    pub fn spawn(
        &mut self,
        request: &Request,
        mode: Mode,
        root: &Path,
        tools: Option<&Tools>,
        deadline: Instant,
        cancel: &AtomicBool,
    ) -> Result<(ManagedGroup, Pump), LaunchError> {
        let (command, id) = self
            .prepare(request, mode, root, tools, deadline, cancel)
            .map_err(LaunchError::Inactive)?;
        let mut group = ManagedGroup::spawn(command, id).map_err(|_| LaunchError::Unconfirmed)?;
        let Some(stdout) = group.take_stdout() else {
            return Err(if group.stop(std::time::Duration::from_secs(2)).is_ok() {
                LaunchError::Inactive(error("WORKER_START_FAILED"))
            } else {
                LaunchError::Unconfirmed
            });
        };
        Ok((group, Pump::new(stdout, id)))
    }

    fn prepare(
        &mut self,
        request: &Request,
        mode: Mode,
        root: &Path,
        tools: Option<&Tools>,
        deadline: Instant,
        cancel: &AtomicBool,
    ) -> Result<(Command, [u8; 16]), CheckError> {
        checkpoint(deadline, cancel)?;
        if ["JAVA_TOOL_OPTIONS", "_JAVA_OPTIONS", "JDK_JAVA_OPTIONS"]
            .iter()
            .any(|key| std::env::var_os(key).is_some_and(|value| !value.is_empty()))
        {
            return Err(error("JAVA_OPTIONS_NOT_SUPPORTED"));
        }
        let mut command = (self.command)();
        crate::automation::java_environment::apply(&mut command, root)?;
        let program = Path::new(command.get_program());
        if !program.is_absolute() {
            return Err(error("EXECUTOR_CHANGED"));
        }
        let selected = if let Some(expected) = &self.executor {
            expected
                .selected(program, &mut || checkpoint(deadline, cancel))
                .map_err(changed)?;
            expected.clone()
        } else {
            let selected =
                Executable::capture("executor", program, &mut || checkpoint(deadline, cancel))
                    .map_err(changed)?;
            self.executor = Some(selected.clone());
            selected
        };
        if let Some(tools) = tools {
            tools.recheck(&mut || checkpoint(deadline, cancel))?;
            tools.pin(&mut command)?;
        }
        let hash = Sha256::digest(root.as_os_str().as_encoded_bytes());
        let id: [u8; 16] = hash[..16]
            .try_into()
            .map_err(|_| error("INVALID_REQUEST"))?;
        write_json(
            &root.join("bootstrap.json"),
            &Bootstrap {
                format_version: request.bootstrap_version(),
                mode,
                request: request.clone(),
                executor: selected,
                tools: tools.cloned(),
            },
        )?;
        command
            .arg("__agent-worker")
            .arg("--root")
            .arg(root)
            .arg("--task-id")
            .arg(hex(&id))
            .env_remove("RUST_LOG");
        // 构造参数之后再次检查，缩小启动前窗口；这仍不是按文件描述符原子执行。
        self.recheck(deadline, cancel)?;
        Ok((command, id))
    }
}

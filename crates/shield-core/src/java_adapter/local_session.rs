//! 普通同步 APK 工具的自有上下文；不替代受控任务的期限、整组停止或最终提交。
use super::{private_io, JavaToolContext};
use anyhow::Result;
use std::path::{Path, PathBuf};

pub(crate) struct LocalSession {
    pub context: JavaToolContext,
    pub java: PathBuf,
    root: PathBuf,
}

impl LocalSession {
    pub fn in_directory(parent: &Path) -> Result<Self> {
        let resource = super::find_java_adapter()?;
        let parent = std::fs::canonicalize(parent)?;
        let root = private_io::directory(&parent)?;
        // 根从创建起归本次普通调用所有，初始化失败也不靠析构清理或换目录重试。
        let context = JavaToolContext::new(&resource, &root)?;
        let java = crate::utils::find_java().map_err(|error| {
            crate::diagnostic::Diagnostic::new(crate::diagnostic::FailureCode::ToolNotFound)
                .attach(error)
        })?;
        context.verify_runtime(&java)?;
        Ok(Self {
            context,
            java,
            root,
        })
    }

    pub fn for_output(output: &Path) -> Result<Self> {
        let output = std::path::absolute(output)?;
        Self::in_directory(output.parent().ok_or_else(super::invalid)?)
    }

    pub fn temporary() -> Result<Self> {
        Self::in_directory(&std::env::temp_dir())
    }

    pub fn finish<T, E>(self, result: std::result::Result<T, E>) -> std::result::Result<T, E> {
        let Self { context, root, .. } = self;
        drop(context);
        finish(&root, result)
    }
}

fn finish<T, E>(root: &Path, result: std::result::Result<T, E>) -> std::result::Result<T, E> {
    // 沿普通同步调用契约，只在完整成功返回后清理本次自有目录；失败／取消保留。
    // 这不是任意 JAR 或脱离标准句柄的 Native 后代停止证明，也没有增加硬超时。
    if result.is_ok() && std::fs::remove_dir_all(root).is_err() {
        log::warn!("APK 工具调用已成功，部分本次临时材料未能清理；未重试业务操作");
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 普通成功清理自有根但失败取消和旁路文件均保留() {
        let parent = tempfile::tempdir().unwrap();
        let original = parent.path().join("原件.apk");
        std::fs::write(&original, b"original").unwrap();
        for success in [false, true] {
            let root = private_io::directory(parent.path()).unwrap();
            let result = finish(
                &root,
                if success {
                    Ok(())
                } else {
                    Err("取消或失败")
                },
            );
            assert_eq!(result.is_ok(), success);
            assert_eq!(root.exists(), !success);
            assert_eq!(std::fs::read(&original).unwrap(), b"original");
        }
    }
}

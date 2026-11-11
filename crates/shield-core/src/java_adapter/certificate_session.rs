//! 普通同步证书操作的私有材料所有者；不用于 apktool、签名或受控任务监督。
use super::{private_io, JavaToolContext};
use anyhow::Result;
use std::path::Path;

pub(crate) fn certificate_session<T>(
    operation: impl FnOnce(&JavaToolContext, &Path) -> Result<T>,
) -> Result<T> {
    let resource = super::find_java_adapter()?;
    let java = crate::utils::find_java()?;
    let temporary = std::fs::canonicalize(std::env::temp_dir())?;
    let workspace = private_io::directory(&temporary)?;
    finish(&workspace, || {
        let context = JavaToolContext::new(&resource, &workspace)?;
        context.verify_runtime(&java)?;
        operation(&context, &java)
    })
}

fn finish<T>(workspace: &Path, operation: impl FnOnce() -> Result<T>) -> Result<T> {
    let result = operation();
    // 固定证书操作没有 apktool Native 后代。只在完整成功返回后清理本次新建材料；
    // 异常、超量或停止未知均保留，不把当前 JVM 退出当成通用整组停止证明。
    if result.is_ok() && std::fs::remove_dir_all(workspace).is_err() {
        log::warn!("证书操作已成功，部分本次临时材料未能清理；未重试业务操作");
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 成功只清理本次根失败保留材料不自动重试() {
        let parent = tempfile::tempdir().unwrap();
        let unrelated = parent.path().join("不相关材料");
        std::fs::write(&unrelated, "原件").unwrap();
        let success = private_io::directory(parent.path()).unwrap();
        finish(&success, || Ok(())).unwrap();
        assert!(!success.exists());
        let failure = private_io::directory(parent.path()).unwrap();
        let result: Result<()> = finish(&failure, || anyhow::bail!("模拟结果未知"));
        assert!(result.is_err());
        assert!(failure.is_dir());
        assert_eq!(std::fs::read(&unrelated).unwrap(), "原件".as_bytes());
    }
}

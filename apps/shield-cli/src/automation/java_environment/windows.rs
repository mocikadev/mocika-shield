//! 复用核心库的 Java 目录别名与真实身份规则，不维护第二份短路径实现。
use super::{invalid, CheckError};
use std::path::{Path, PathBuf};

pub(super) fn ascii_directory(root: &Path) -> Result<PathBuf, CheckError> {
    shield_core::java_directory::ascii_directory(root).map_err(|_| invalid())
}

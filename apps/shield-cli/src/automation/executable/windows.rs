//! Windows 原生执行身份适配，不把扩展名检查当作 PE 格式或可启动性的保证。
use super::{error, CheckError};
use std::path::Path;
pub(in crate::automation) type Stamp = crate::automation::task_files::FileStamp;

pub(super) fn read(path: &Path) -> Result<Stamp, CheckError> {
    // 内部入口只接受 shield/java/keytool 的原生 exe，不隐式转交 cmd 或脚本解释器。
    if !path
        .extension()
        .is_some_and(|value| value.eq_ignore_ascii_case("exe"))
    {
        return Err(error("EXECUTABLE_CHANGED"));
    }
    read_regular(path)
}

pub(super) fn read_regular(path: &Path) -> Result<Stamp, CheckError> {
    crate::automation::task_files::file_stamp(path).map_err(|_| error("EXECUTABLE_CHANGED"))
}

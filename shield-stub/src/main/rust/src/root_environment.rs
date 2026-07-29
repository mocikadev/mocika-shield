//! 高置信 Root 环境检测。弱信号不单独触发拒绝，避免工控机与测试设备误报。

use std::fs;
use std::io::{BufRead, BufReader};
use std::os::unix::fs::PermissionsExt;

#[cfg(target_os = "android")]
use std::ffi::CStr;
#[cfg(target_os = "android")]
use std::os::raw::{c_char, c_int};

#[cfg(target_os = "android")]
extern "C" {
    fn __system_property_get(name: *const c_char, value: *mut c_char) -> c_int;
}

const SU_PATHS: &[&str] = &[
    "/system/bin/su",
    "/system/xbin/su",
    "/sbin/su",
    "/vendor/bin/su",
    "/data/adb/ksu/bin/su",
];

const INJECTION_SIGNATURES: &[&str] = &["magisk", "zygisk", "kernelsu", "apatch"];

pub fn check() -> bool {
    has_executable_su() || has_privileged_uid() || has_root_adb() || has_root_injection_trace()
}

#[cfg(target_os = "android")]
fn has_root_adb() -> bool {
    const PROPERTY_NAME: &[u8] = b"service.adb.root\0";
    // Android 系统属性值上限为 91 字节，额外保留结尾零字节。
    let mut value = [0 as c_char; 92];
    let length = unsafe {
        __system_property_get(PROPERTY_NAME.as_ptr().cast::<c_char>(), value.as_mut_ptr())
    };
    if length <= 0 {
        return false;
    }
    let value = unsafe { CStr::from_ptr(value.as_ptr()) };
    value.to_str().map(is_root_adb_value).unwrap_or(false)
}

#[cfg(not(target_os = "android"))]
fn has_root_adb() -> bool {
    false
}

fn is_root_adb_value(value: &str) -> bool {
    value == "1"
}

fn has_executable_su() -> bool {
    SU_PATHS.iter().any(|path| {
        fs::metadata(path)
            .map(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
            .unwrap_or(false)
    })
}

fn has_privileged_uid() -> bool {
    let Ok(status) = fs::read_to_string("/proc/self/status") else {
        return false;
    };
    effective_uid(&status) == Some(0)
}

fn effective_uid(status: &str) -> Option<u32> {
    status.lines().find_map(|line| {
        let values = line.strip_prefix("Uid:")?;
        values.split_whitespace().nth(1)?.parse().ok()
    })
}

fn has_root_injection_trace() -> bool {
    ["/proc/self/maps", "/proc/self/mountinfo", "/proc/mounts"]
        .iter()
        .any(|path| file_contains_signature(path))
}

fn file_contains_signature(path: &str) -> bool {
    let Ok(file) = fs::File::open(path) else {
        return false;
    };
    BufReader::new(file)
        .lines()
        .map_while(Result::ok)
        .any(|line| {
            let lower = line.to_ascii_lowercase();
            INJECTION_SIGNATURES
                .iter()
                .any(|signature| lower.contains(signature))
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 读取有效用户标识() {
        assert_eq!(
            effective_uid("Name:\tapp\nUid:\t10123\t10123\t10123\t10123\n"),
            Some(10123)
        );
        assert_eq!(effective_uid("Uid:\t0\t0\t0\t0\n"), Some(0));
        assert_eq!(effective_uid("Name:\tapp\n"), None);
    }

    #[test]
    fn 仅明确开启的_adb_root_属性命中() {
        assert!(is_root_adb_value("1"));
        assert!(!is_root_adb_value("0"));
        assert!(!is_root_adb_value(""));
        assert!(!is_root_adb_value("true"));
    }
}

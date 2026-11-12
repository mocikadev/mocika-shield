//! Linux WebView 启动兼容；必须在创建 Tauri／GTK 运行时之前调用。
use std::ffi::OsStr;

const VARIABLE: &str = "WEBKIT_DISABLE_DMABUF_RENDERER";

fn default_override(linux: bool, existing: Option<&OsStr>) -> Option<&'static str> {
    (linux && existing.is_none()).then_some("1")
}

pub(crate) fn configure() {
    let current = std::env::var_os(VARIABLE);
    if let Some(value) = default_override(cfg!(target_os = "linux"), current.as_deref()) {
        // 仅设置本进程启动环境；显式值（包括空串）都归用户控制，不改系统配置。
        std::env::set_var(VARIABLE, value);
    }
}

pub(crate) fn diagnostic() -> &'static str {
    if !cfg!(target_os = "linux") {
        return "不适用";
    }
    match std::env::var_os(VARIABLE).as_deref() {
        Some(value) if value == "1" => "已禁用 DMA-BUF 渲染路径",
        Some(_) => "用户显式覆盖",
        None => "系统默认",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 默认仅作用于未设置变量的_linux() {
        assert_eq!(default_override(true, None), Some("1"));
        assert_eq!(default_override(false, None), None);
        for value in ["", "0", "1", "用户值"] {
            assert_eq!(default_override(true, Some(OsStr::new(value))), None);
            assert_eq!(default_override(false, Some(OsStr::new(value))), None);
        }
    }

    #[test]
    fn 子进程验证真实环境且不污染测试进程() {
        const MARKER: &str = "MOCIKA_WEBVIEW_TEST_CASE";
        if let Ok(case) = std::env::var(MARKER) {
            configure();
            let expected = if case == "missing" {
                cfg!(target_os = "linux").then(|| "1".into())
            } else {
                Some(case.into())
            };
            assert_eq!(std::env::var_os(VARIABLE), expected);
            assert!(!diagnostic().contains("任意用户值"));
            return;
        }
        let before = std::env::var_os(VARIABLE);
        for case in ["missing", "", "0", "1", "任意用户值"] {
            let mut child = std::process::Command::new(std::env::current_exe().unwrap());
            child.args(["子进程验证真实环境且不污染测试进程", "--test-threads=1"]);
            child.env(MARKER, case);
            if case == "missing" {
                child.env_remove(VARIABLE);
            } else {
                child.env(VARIABLE, case);
            }
            let output = child.output().unwrap();
            assert!(output.status.success(), "子进程启动策略验证失败");
        }
        assert_eq!(std::env::var_os(VARIABLE), before);
    }
}

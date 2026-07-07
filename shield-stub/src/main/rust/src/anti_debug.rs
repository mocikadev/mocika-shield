/// 反调试检测：通过 /proc 文件系统检测 ptrace 调试器与 Frida 注入。
/// 所有检测均基于标准库文件读取，无额外 Cargo 依赖。
use std::fs;
use std::io::{BufRead, BufReader};

/// Frida 注入到目标进程后必然出现在内存映射中的库名特征。
const FRIDA_MAP_SIGS: &[&str] = &[
    "frida-agent",
    "frida-gadget",
    "libfrida",
    "gum-js-loop", // Frida Gum JS 引擎线程段
];

/// Frida 依赖的 GLib 内部线程名特征。
/// phantom-frida 等工具重命名了 .so 文件名，但无法重命名 GLib 线程，此处作为补充检测。
const FRIDA_THREAD_SIGS: &[&str] = &[
    "gmain",      // GLib 主循环线程
    "gdbus",      // D-Bus 通信线程（Frida 进程间通信依赖）
    "pool-frida", // Frida 线程池
    "gum-js-loop",
];

/// 综合反调试检测入口。任一检测命中即返回 true。
/// 检测顺序：TracerPid（最快，单次读取）→ maps 扫描 → 线程名扫描（兜底）。
pub fn check() -> bool {
    check_tracer_pid() || check_frida_maps() || check_frida_threads()
}

/// 检测 ptrace 类调试器（Android Studio / IDA / lldb 等）。
/// 读取 /proc/self/status 中的 TracerPid 字段：非零表示已有进程通过 ptrace 附加。
fn check_tracer_pid() -> bool {
    let file = match fs::File::open("/proc/self/status") {
        Ok(f) => f,
        Err(_) => return false,
    };
    for line in BufReader::new(file).lines() {
        let Ok(line) = line else { continue };
        if line.starts_with("TracerPid") {
            return line
                .split(':')
                .nth(1)
                .and_then(|s| s.trim().parse::<u32>().ok())
                .map(|pid| pid != 0)
                .unwrap_or(false);
        }
    }
    false
}

/// 检测 Frida 注入：扫描 /proc/self/maps 中的 Frida 库特征字符串。
/// Frida 注入后必然将 frida-agent.so / frida-gadget.so 加载进进程地址空间，
/// 这些映射条目无法对自身进程隐藏。
fn check_frida_maps() -> bool {
    let file = match fs::File::open("/proc/self/maps") {
        Ok(f) => f,
        Err(_) => return false,
    };
    for line in BufReader::new(file).lines() {
        let Ok(line) = line else { continue };
        for sig in FRIDA_MAP_SIGS {
            if line.contains(sig) {
                return true;
            }
        }
    }
    false
}

/// 辅助检测：遍历 /proc/self/task/*/comm，匹配 Frida 依赖的 GLib 线程名。
/// 作为 maps 扫描的兜底：覆盖重命名了库文件但无法重命名内部 GLib 线程的变种注入工具。
fn check_frida_threads() -> bool {
    let task_dir = match fs::read_dir("/proc/self/task") {
        Ok(d) => d,
        Err(_) => return false,
    };
    for entry in task_dir.flatten() {
        let comm_path = entry.path().join("comm");
        let Ok(comm) = fs::read_to_string(&comm_path) else {
            continue;
        };
        let comm = comm.trim();
        for sig in FRIDA_THREAD_SIGS {
            if comm.contains(sig) {
                return true;
            }
        }
    }
    false
}

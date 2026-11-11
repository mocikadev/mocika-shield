//! Ctrl+Break 只发送到本夹具私有控制台中新建的进程组，绝不广播到用户控制台。
use super::*;
use std::io::Read;
use std::os::windows::process::CommandExt;
use windows_sys::Win32::System::{
    Console::{
        AllocConsole, FreeConsole, GenerateConsoleCtrlEvent, GetConsoleProcessList,
        CTRL_BREAK_EVENT,
    },
    Threading::CREATE_NEW_PROCESS_GROUP,
};

#[test]
fn 私有控制台break取消会停止本次普通命令后代() {
    run_console("signal");
}
#[cfg(feature = "mcp-server")]
#[test]
fn 私有控制台中断同时验证异步监督器取消() {
    run_console("async-signal");
}
fn run_console(mode: &str) {
    let mut root = tempfile::tempdir().unwrap();
    root.disable_cleanup(true);
    let error = fs::File::create(root.path().join("owner-error")).unwrap();
    let mut owner = ChildGuard(
        command(root.path(), "console-owner")
            .env("SHIELD_TEST_CONSOLE_CHILD_MODE", mode)
            .stderr(error)
            .spawn()
            .unwrap(),
    );
    let mut status = None;
    wait_for(|| {
        status = owner.0.try_wait().unwrap();
        status.is_some()
    });
    assert!(
        status.unwrap().success(),
        "私有控制台夹具失败：状态={:?}，阶段={}，本次诊断={}，子级诊断={}",
        status.unwrap().code(),
        fs::read_to_string(root.path().join("console-stage")).unwrap_or_default(),
        diagnostic(root.path(), "owner-error"),
        diagnostic(root.path(), "signal-error")
    );
    assert_eq!(
        fs::read_to_string(root.path().join("cause")).unwrap(),
        "signal"
    );
    assert!(root.path().join("stopped").exists());
    root.disable_cleanup(false);
}

pub(super) fn console_owner(root: &Path) {
    let mut process = 0;
    let attached = unsafe { GetConsoleProcessList(&mut process, 1) };
    fs::write(root.join("console-initial"), attached.to_string()).unwrap();
    if attached != 0 {
        // 仅脱离本次隔离夹具自身的旧绑定，不释放其他进程的引用，也不向旧控制台发送事件。
        fs::write(root.join("console-stage"), "detach-self").unwrap();
        assert_ne!(
            unsafe { FreeConsole() },
            0,
            "夹具不能解除自己的旧绑定：{}",
            std::io::Error::last_os_error()
        );
    }
    fs::write(root.join("console-stage"), "allocate").unwrap();
    assert_ne!(
        unsafe { AllocConsole() },
        0,
        "夹具必须拥有新控制台，不借用用户控制台：{}",
        std::io::Error::last_os_error()
    );
    fs::write(root.join("console-stage"), "spawn").unwrap();
    // 仅此隔离测试覆盖无窗口默认值，验证真实控制台事件；产品启动仍不创建控制台。
    let mut child = ChildGuard(
        command(
            root,
            &std::env::var("SHIELD_TEST_CONSOLE_CHILD_MODE").unwrap_or_else(|_| "signal".into()),
        )
        .creation_flags(CREATE_NEW_PROCESS_GROUP)
        .stdout(Stdio::null())
        .stderr(fs::File::create(root.join("signal-error")).unwrap())
        .spawn()
        .unwrap(),
    );
    fs::write(root.join("console-stage"), "wait-ready").unwrap();
    wait_for(|| root.join("ready").exists());
    fs::write(root.join("console-stage"), "deliver").unwrap();
    assert_ne!(
        unsafe { GenerateConsoleCtrlEvent(CTRL_BREAK_EVENT, child.0.id()) },
        0,
        "私有控制台发送失败：{}",
        std::io::Error::last_os_error()
    );
    fs::write(root.join("console-stage"), "verify").unwrap();
    verify(root, &mut child.0, "signal");
    fs::write(root.join("console-stage"), "done").unwrap();
    drop(child);
    // 本夹具改变了自身控制台，不能再让 libtest 向缓存的旧标准输出打印回执。
    std::process::exit(0);
}

fn diagnostic(root: &Path, name: &str) -> String {
    let mut bytes = Vec::new();
    if let Ok(file) = fs::File::open(root.join(name)) {
        let _ = file.take(4096).read_to_end(&mut bytes);
    }
    String::from_utf8_lossy(&bytes).into_owned()
}

//! 创建时 Job 归属和白名单继承的真实验证，只运行本仓库测试夹具。
use super::*;
use crate::automation::native_group::tests::{fixture, ordinary, ParentFixture};
use std::{fs, mem::size_of, path::Path};
use windows_sys::Win32::{
    Security::SECURITY_ATTRIBUTES,
    System::Threading::{
        CreateEventW, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE,
    },
};

fn wait_for(mut ready: impl FnMut() -> bool) {
    let end = Instant::now() + Duration::from_secs(5);
    while !ready() {
        assert!(Instant::now() < end);
        std::thread::sleep(Duration::from_millis(5));
    }
}
fn writer(root: &Path) -> OwnedHandle {
    let pid = fs::read_to_string(root.join("child-pid"))
        .unwrap()
        .parse::<u32>()
        .unwrap();
    let raw = unsafe {
        OpenProcess(
            PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE,
            0,
            pid,
        )
    };
    assert!(!raw.is_null());
    unsafe { OwnedHandle::from_raw_handle(raw) }
}

#[test]
fn 入口立即创建的后代已在本次job且主进程早退不影响停止证明() {
    let mut root = tempfile::tempdir().unwrap();
    root.disable_cleanup(true);
    let mut group = ManagedGroup::spawn_command(ordinary("early-exit", root.path())).unwrap();
    group.close_input();
    wait_for(|| {
        root.path()
            .join("heartbeat")
            .metadata()
            .is_ok_and(|v| v.len() > 0)
            && group.leader_exited().unwrap()
    });
    let writer = writer(root.path());
    assert!(group.job.contains(&writer).unwrap());
    let deadline = Instant::now() + Duration::from_secs(2);
    group
        .stop(deadline.saturating_duration_since(Instant::now()))
        .unwrap();
    assert_eq!(
        // 计数归零和内核对象发出退出信号不是同一瞬间，两者仍用原来的总收尾期限。
        unsafe {
            WaitForSingleObject(
                writer.as_raw_handle(),
                deadline
                    .saturating_duration_since(Instant::now())
                    .as_millis() as u32,
            )
        },
        WAIT_OBJECT_0
    );
    assert_eq!(group.exit_status().unwrap().code(), Some(17));
    root.disable_cleanup(false);
}

#[test]
fn 普通命令持有者被强制结束时最后job句柄关闭停止后代() {
    let mut root = tempfile::tempdir().unwrap();
    root.disable_cleanup(true);
    let mut parent = ParentFixture(
        fixture("plain-parent", root.path())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    wait_for(|| root.path().join("parent-ready").exists());
    let writer = writer(root.path());
    parent.0.kill().unwrap();
    parent.0.wait().unwrap();
    assert_eq!(
        unsafe { WaitForSingleObject(writer.as_raw_handle(), 2000) },
        WAIT_OBJECT_0
    );
    root.disable_cleanup(false);
}

#[test]
fn 白名单不继承额外句柄且正常退出码259不误判仍活跃() {
    let root = tempfile::tempdir().unwrap();
    let security = SECURITY_ATTRIBUTES {
        nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: std::ptr::null_mut(),
        bInheritHandle: 1,
    };
    let raw = unsafe { CreateEventW(&security, 1, 0, std::ptr::null()) };
    assert!(!raw.is_null());
    let event = unsafe { OwnedHandle::from_raw_handle(raw) };
    let mut spec = ordinary("handle-check", root.path());
    spec.set_env(
        "SHIELD_TEST_EVENT",
        std::ffi::OsStr::new(&(raw as usize).to_string()),
    );
    let mut group = ManagedGroup::spawn_command(spec).unwrap();
    group.close_input();
    wait_for(|| group.leader_exited().unwrap());
    group.stop(Duration::from_secs(2)).unwrap();
    assert_eq!(group.exit_status().unwrap().code(), Some(0));
    assert_eq!(
        unsafe { WaitForSingleObject(event.as_raw_handle(), 0) },
        WAIT_TIMEOUT
    );
    let mut group = ManagedGroup::spawn_command(ordinary("exit-259", root.path())).unwrap();
    group.close_input();
    wait_for(|| group.leader_exited().unwrap());
    group.stop(Duration::from_secs(2)).unwrap();
    assert_eq!(group.exit_status().unwrap().code(), Some(259));
}

#[test]
fn 输出阻塞不阻止job停止而无效参数不启动子进程() {
    let mut root = tempfile::tempdir().unwrap();
    root.disable_cleanup(true);
    let mut group = ManagedGroup::spawn_command(ordinary("flood", root.path())).unwrap();
    let _unread = group.take_stdout().unwrap();
    wait_for(|| root.path().join("flood-ready").exists());
    group.stop(Duration::from_secs(2)).unwrap();
    for mode in ["nul", "duplicate", "script"] {
        let mut spec = ordinary("plain-echo", root.path());
        match mode {
            "nul" => spec.arguments.push("bad\0argument".into()),
            "duplicate" => spec.environment.extend([
                ("重复Å".into(), "one".into()),
                ("重复å".into(), "two".into()),
            ]),
            _ => {
                spec.program.set_extension("cmd");
            }
        };
        assert!(ManagedGroup::spawn_command(spec).is_err());
    }
    assert!(!root.path().join("arguments").exists());
    root.disable_cleanup(false);
}

//! 取消输入／输出错误在独立夹具中验证，不在测试宿主登记控制台处理器或读取 stdin。
use super::*;
use crate::automation::native_group::{tests::ordinary, ManagedGroup};
use std::{
    fs,
    io::Write,
    path::Path,
    process::{Child, Command, Stdio},
    time::Instant,
};

const ENTRY: &str = "automation::cancellation::process_tests::外部取消内部夹具";

pub(super) fn command(root: &Path, mode: &str) -> Command {
    let mut command = shield_core::utils::no_window_command(std::env::current_exe().unwrap());
    command
        .args([
            "--ignored",
            "--exact",
            ENTRY,
            "--nocapture",
            "--test-threads=1",
        ])
        .env("SHIELD_TEST_CANCEL_ROOT", root)
        .env("SHIELD_TEST_CANCEL_MODE", mode)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::from(fs::File::create(root.join("stderr")).unwrap()));
    command
}

pub(super) fn wait_for(mut predicate: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(8);
    while !predicate() {
        assert!(Instant::now() < deadline, "取消夹具未在期限内完成");
        std::thread::sleep(Duration::from_millis(5));
    }
}

pub(super) struct ChildGuard(pub Child);
impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

pub(super) fn verify(root: &Path, child: &mut Child, cause: &str) {
    let mut status = None;
    wait_for(|| {
        status = child.try_wait().unwrap();
        status.is_some()
    });
    if cause == "output_closed" {
        assert_eq!(
            status.unwrap().code(),
            Some(3),
            "无回执不能返回成功；自有夹具诊断：{}",
            fs::read_to_string(root.join("stderr")).unwrap_or_default()
        );
    } else {
        assert!(status.unwrap().success(), "取消夹具失败，保留材料");
    }
    assert_eq!(fs::read_to_string(root.join("cause")).unwrap(), cause);
    assert!(root.join("stopped").exists());
    let before = fs::read(root.join("heartbeat")).unwrap();
    std::thread::sleep(Duration::from_millis(80));
    assert_eq!(before, fs::read(root.join("heartbeat")).unwrap());
}

#[test]
fn stdin取消断开和畸形帧均先停止普通命令后代() {
    for (input, expected) in [
        (Some(b"{\"type\":\"cancel\"}\n".as_slice()), "stdin"),
        (None, "stdin_closed"),
        (Some(b"invalid\n".as_slice()), "invalid_control"),
    ] {
        let mut root = tempfile::tempdir().unwrap();
        root.disable_cleanup(true);
        let mut child = ChildGuard(command(root.path(), "input").spawn().unwrap());
        wait_for(|| root.path().join("ready").exists());
        if let Some(bytes) = input {
            child.0.stdin.as_mut().unwrap().write_all(bytes).unwrap();
        } else {
            child.0.stdin.take();
        }
        verify(root.path(), &mut child.0, expected);
        root.disable_cleanup(false);
    }
}

#[test]
fn 真实输出写入失败触发同一取消且不误称任务成功() {
    let mut root = tempfile::tempdir().unwrap();
    root.disable_cleanup(true);
    let mut child = ChildGuard(command(root.path(), "output").spawn().unwrap());
    wait_for(|| root.path().join("ready").exists());
    child.0.stdout.take();
    fs::write(root.path().join("write-now"), b"test").unwrap();
    verify(root.path(), &mut child.0, "output_closed");
    root.disable_cleanup(false);
}

#[test]
fn 实际标准输出无人读取也按期取消并停止后代() {
    let mut root = tempfile::tempdir().unwrap();
    root.disable_cleanup(true);
    let mut child = ChildGuard(command(root.path(), "output-blocked").spawn().unwrap());
    wait_for(|| root.path().join("ready").exists());
    let _unread = child.0.stdout.take().unwrap();
    fs::write(root.path().join("write-now"), b"test").unwrap();
    verify(root.path(), &mut child.0, "output_closed");
    root.disable_cleanup(false);
}

#[cfg(all(windows, feature = "mcp-server"))]
#[test]
fn 异步标准流真实输入断开和写入失败均停止普通命令后代() {
    for (mode, cause) in [
        ("async-input", "stdin_closed"),
        ("async-output", "output_closed"),
    ] {
        let mut root = tempfile::tempdir().unwrap();
        root.disable_cleanup(true);
        let mut child = ChildGuard(command(root.path(), mode).spawn().unwrap());
        wait_for(|| root.path().join("ready").exists());
        if mode == "async-input" {
            child.0.stdin.take();
        } else {
            child.0.stdout.take();
            fs::write(root.path().join("write-now"), b"test").unwrap();
        }
        verify(root.path(), &mut child.0, cause);
        root.disable_cleanup(false);
    }
}

#[test]
#[ignore = "内部取消夹具，仅由隔离父测试启动"]
fn 外部取消内部夹具() {
    let Ok(root) = std::env::var("SHIELD_TEST_CANCEL_ROOT") else {
        return;
    };
    let root = Path::new(&root);
    let mode = std::env::var("SHIELD_TEST_CANCEL_MODE").unwrap();
    #[cfg(windows)]
    if mode == "console-owner" {
        windows::console_owner(root);
        return;
    }
    #[cfg(all(windows, feature = "mcp-server"))]
    if mode.starts_with("async-") {
        crate::automation::transport::run_fixture(root, &mode);
        return;
    }
    let controls = Controls::new(mode == "input").unwrap();
    let mut group = ManagedGroup::spawn_command(ordinary("writer", root)).unwrap();
    group.close_input();
    wait_for(|| root.join("heartbeat").metadata().is_ok_and(|v| v.len() > 0));
    fs::write(root.join("ready"), b"test").unwrap();
    let deadline = Instant::now() + Duration::from_secs(8);
    let mut output = None;
    if mode.starts_with("output") {
        wait_for(|| root.join("write-now").exists());
        let queue = crate::automation::event_output::Output::new(
            std::io::stdout(),
            controls.clone_signal(),
        )
        .ok()
        .unwrap();
        let since = Instant::now();
        if mode == "output-blocked" {
            // 总帧数少于队列容量，不能通过队列满伪造阻塞写期限已触发。
            for _ in 0..31 {
                assert!(queue.send(vec![b'x'; 4097]));
            }
        } else {
            assert!(queue.send(b"finite probe\n".to_vec()));
        }
        output = Some((queue, since));
    }
    while !controls.cancel().load(Ordering::SeqCst) {
        assert!(Instant::now() < deadline, "未收到取消，不伪造成功");
        std::thread::sleep(Duration::from_millis(5));
    }
    group.stop(Duration::from_secs(2)).unwrap();
    assert!(group.leader_exited().unwrap());
    if let Some((queue, since)) = output {
        if mode == "output-blocked" {
            assert!(since.elapsed() >= Duration::from_secs(1));
        }
        assert!(!queue.finish(Duration::from_millis(100)));
    }
    let cause = match controls.cause() {
        Cause::Input => "stdin",
        Cause::Closed => "stdin_closed",
        Cause::Invalid => "invalid_control",
        Cause::Signal => "signal",
        Cause::Output => "output_closed",
        Cause::None => panic!("丢失取消原因"),
    };
    fs::write(root.join("cause"), cause).unwrap();
    fs::write(root.join("stopped"), b"test").unwrap();
    if mode.starts_with("output") {
        // 断开的 libtest 输出不能再打印测试回执；停止确认后仍用无回执退出码结束。
        drop(controls);
        std::process::exit(3);
    }
}

#[cfg(windows)]
#[path = "process_tests/windows.rs"]
mod windows;

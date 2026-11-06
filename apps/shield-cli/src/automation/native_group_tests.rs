//! 本机真实进程回归，只运行本测试二进制，不执行 Java、APK 或用户命令。
use super::*;
use crate::automation::task_lifecycle::{Status, Task};
use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::path::Path;

const TASK_ID: [u8; 16] = [4; 16];
const PLAN: [u8; 32] = [7; 32];
const ENTRY: &str = "automation::native_group::tests::原生子进程夹具";

struct ParentFixture(Child);
impl Drop for ParentFixture {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn fixture(role: &str, root: &Path) -> Command {
    let mut command = shield_core::utils::no_window_command(std::env::current_exe().unwrap());
    command
        .env_remove("MOCIKA_SHIELD_KS_PASS")
        .env_remove("MOCIKA_SHIELD_KEY_PASS")
        .env_remove("MOCIKA_SHIELD_TOOL_KS_PASS")
        .env_remove("MOCIKA_SHIELD_TOOL_KEY_PASS")
        .args([
            "--ignored",
            "--exact",
            ENTRY,
            "--nocapture",
            "--test-threads=1",
        ])
        .env("MOCIKA_TEST_GROUP_ROLE", role)
        .env("MOCIKA_TEST_GROUP_ROOT", root);
    command
}

fn wait_for(mut predicate: impl FnMut() -> bool) {
    let end = Instant::now() + Duration::from_secs(5);
    while !predicate() {
        assert!(Instant::now() < end, "原生夹具未在期限内就绪");
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn record(root: &Path, name: &str, bytes: &[u8]) {
    let pending = root.join(format!(".{name}.pending"));
    fs::write(&pending, bytes).unwrap();
    fs::rename(pending, root.join(name)).unwrap();
}

fn started(root: &Path) -> ManagedGroup {
    let mut group = ManagedGroup::spawn(fixture("worker", root), TASK_ID).unwrap();
    wait_for(|| root.join("worker-ready").exists());
    assert!(!root.join("work-started").exists());
    group.start(PLAN).unwrap();
    wait_for(|| {
        root.join("stdin-empty").exists()
            && root.join("heartbeat").metadata().is_ok_and(|m| m.len() > 0)
    });
    group
}

#[test]
fn 未授权开始前不执行工作且停止后禁止复用() {
    let root = tempfile::tempdir().unwrap();
    let mut group = ManagedGroup::spawn(fixture("worker", root.path()), TASK_ID).unwrap();
    let _stdout = group.take_stdout().unwrap();
    wait_for(|| root.path().join("worker-ready").exists());
    assert!(!root.path().join("work-started").exists());
    group.stop(Duration::from_secs(2)).unwrap();
    assert!(!macos::process_running(group.id() as i32, group.id() as i32).unwrap());
    assert_eq!(group.start(PLAN), Err(ProcessError::Closed));
    group.stop(Duration::from_secs(2)).unwrap();
}

#[test]
fn 取消及超时均停止真实后代写入后才报告终态() {
    for expired in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let now = Instant::now();
        let mut task = Task::new(
            now,
            Duration::from_secs(if expired { 1 } else { 5 }),
            PLAN,
            [0; 32],
        )
        .unwrap();
        let mut group = ManagedGroup::spawn(fixture("worker", root.path()), TASK_ID).unwrap();
        task.isolation_confirmed(Instant::now()).unwrap();
        wait_for(|| root.path().join("worker-ready").exists());
        task.hello(Instant::now()).unwrap();
        task.start(Instant::now()).unwrap();
        group.start(PLAN).unwrap();
        wait_for(|| {
            root.path().join("stdin-empty").exists()
                && root
                    .path()
                    .join("heartbeat")
                    .metadata()
                    .is_ok_and(|m| m.len() > 0)
        });
        loop {
            let observed = Instant::now();
            if !expired && now.elapsed() >= Duration::from_millis(50) {
                task.cancel(observed).unwrap();
            } else {
                let _ = task.checkpoint(observed);
            }
            if task.stop_reason().is_some() {
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(task.take_outcome().is_none());
        assert!(!task.can_cleanup());
        group.stop(Duration::from_secs(2)).unwrap();
        task.tree_stopped(Instant::now()).unwrap();
        assert_eq!(
            task.take_outcome().unwrap().status,
            if expired {
                Status::TimedOut
            } else {
                Status::Cancelled
            }
        );
        assert!(task.can_cleanup());
        let heartbeat = root.path().join("heartbeat");
        let stopped = fs::read(&heartbeat).unwrap();
        std::thread::sleep(Duration::from_millis(80));
        assert_eq!(fs::read(heartbeat).unwrap(), stopped);
    }
}

#[test]
fn 工作进程意外退出仍能停止其后代() {
    let root = tempfile::tempdir().unwrap();
    let mut group = started(root.path());
    group.child.kill().unwrap();
    // 这里故意不 wait，组长 PID 仍被保留；后代不能因组长退出逃过收尾。
    group.stop(Duration::from_secs(2)).unwrap();
    let heartbeat = root.path().join("heartbeat");
    let stopped = fs::read(&heartbeat).unwrap();
    std::thread::sleep(Duration::from_millis(80));
    assert_eq!(fs::read(heartbeat).unwrap(), stopped);
}

#[test]
fn 异常离开父作用域自动停止本次组() {
    let root = tempfile::tempdir().unwrap();
    let pid;
    {
        let group = started(root.path());
        pid = group.id() as i32;
    }
    assert!(!macos::process_running(pid, pid).unwrap());
    let heartbeat = root.path().join("heartbeat");
    let stopped = fs::read(&heartbeat).unwrap();
    std::thread::sleep(Duration::from_millis(80));
    assert_eq!(fs::read(heartbeat).unwrap(), stopped);
}

#[test]
fn 父进程异常退出由租约停止仍存活的工作组() {
    let root = tempfile::tempdir().unwrap();
    let mut parent = ParentFixture(
        fixture("parent", root.path())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    wait_for(|| root.path().join("parent-ready").exists());
    let pid: i32 = fs::read_to_string(root.path().join("worker-ready"))
        .unwrap()
        .parse()
        .unwrap();
    parent.0.kill().unwrap();
    parent.0.wait().unwrap();
    wait_for(|| !macos::process_running(pid, pid).unwrap());
    let child: i32 = fs::read_to_string(root.path().join("child-pid"))
        .unwrap()
        .parse()
        .unwrap();
    wait_for(|| !macos::process_running(child, pid).unwrap());
    let heartbeat = root.path().join("heartbeat");
    let stopped = fs::read(&heartbeat).unwrap();
    std::thread::sleep(Duration::from_millis(80));
    assert_eq!(fs::read(heartbeat).unwrap(), stopped);
}

#[test]
fn 父端断开及重复开始都停止本次工作组() {
    for malformed in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let mut group = started(root.path());
        if malformed {
            // 直接破坏私有管道仅用于反向测试，不作为产品接口。
            let mut control = group.control.take().unwrap();
            control.send(Message::Start { plan_digest: PLAN }).unwrap();
        } else {
            group.control.take();
        }
        wait_for(|| !macos::process_running(group.id() as i32, group.id() as i32).unwrap());
        group.stop(Duration::from_secs(2)).unwrap();
    }
}

#[test]
#[ignore = "内部子进程夹具，仅由原生控制测试隔离启动，不是独立验收项"]
fn 原生子进程夹具() {
    let Ok(role) = std::env::var("MOCIKA_TEST_GROUP_ROLE") else {
        return;
    };
    let root = std::path::PathBuf::from(std::env::var_os("MOCIKA_TEST_GROUP_ROOT").unwrap());
    match role.as_str() {
        "worker" => {
            let lease = WorkerLease::install(TASK_ID).unwrap();
            record(
                &root,
                "worker-ready",
                std::process::id().to_string().as_bytes(),
            );
            assert_eq!(lease.wait_start(Duration::from_secs(10)).unwrap(), PLAN);
            record(&root, "work-started", b"yes");
            let mut child = fixture("writer", &root)
                .stdin(Stdio::inherit())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .unwrap();
            record(&root, "child-pid", child.id().to_string().as_bytes());
            std::thread::spawn(move || {
                let _ = child.wait();
            });
            loop {
                std::thread::sleep(Duration::from_millis(20));
            }
        }
        "writer" => {
            let mut byte = [0u8; 1];
            assert_eq!(std::io::stdin().read(&mut byte).unwrap(), 0);
            record(&root, "stdin-empty", b"yes");
            let mut file = OpenOptions::new()
                .create(true)
                .append(true)
                .open(root.join("heartbeat"))
                .unwrap();
            loop {
                file.write_all(b".").unwrap();
                std::thread::sleep(Duration::from_millis(5));
            }
        }
        "parent" => {
            let _group = started(&root);
            record(&root, "parent-ready", b"yes");
            loop {
                std::thread::sleep(Duration::from_millis(20));
            }
        }
        _ => panic!("未知夹具角色"),
    }
}

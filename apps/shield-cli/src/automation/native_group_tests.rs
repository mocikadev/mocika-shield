//! 本机真实进程回归；默认只运行测试二进制，正式 Java 助手另由显式专项验证。
#[cfg(windows)]
use super::platform::process_running;
#[cfg(unix)]
use super::query::process_running;
use super::*;
use crate::automation::task_lifecycle::{Status, Task};
use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::Instant;

const TASK_ID: [u8; 16] = [4; 16];
const PLAN: [u8; 32] = [7; 32];
const ENTRY: &str = "automation::native_group::tests::原生子进程夹具";

#[path = "native_group_apktool_tests.rs"]
mod apktool_tests;
#[path = "native_group_certificate_fixture.rs"]
mod certificate_fixture;
#[path = "native_group_certificate_tests.rs"]
mod certificate_tests;
#[path = "native_group_java_tests.rs"]
mod java_tests;
#[path = "native_group_signing_tests.rs"]
mod signing_tests;

fn test_root() -> tempfile::TempDir {
    let mut root = tempfile::tempdir().unwrap();
    // 停止证明和全部断言通过前保留材料，不能删除可能仍被后代使用的目录。
    root.disable_cleanup(true);
    root
}

pub(super) struct ParentFixture(pub(super) Child);
impl Drop for ParentFixture {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

pub(super) fn fixture(role: &str, root: &Path) -> Command {
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

pub(in crate::automation) fn ordinary(role: &str, root: &Path) -> CommandSpec {
    let command = fixture(role, root);
    let mut spec = CommandSpec::new(
        Path::new(command.get_program()),
        command.get_args().map(|v| v.to_owned()).collect(),
        root,
    );
    for name in [
        "MOCIKA_SHIELD_KS_PASS",
        "MOCIKA_SHIELD_KEY_PASS",
        "MOCIKA_SHIELD_TOOL_KS_PASS",
        "MOCIKA_SHIELD_TOOL_KEY_PASS",
    ] {
        spec.remove_env(name);
    }
    spec.set_env("MOCIKA_TEST_GROUP_ROLE", std::ffi::OsStr::new(role));
    spec.set_env("MOCIKA_TEST_GROUP_ROOT", root.as_os_str());
    spec
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
    // 连续停止短生命周期工作进程，覆盖组归零与句柄退出信号的先后时序。
    for _ in 0..16 {
        let mut root = test_root();
        let mut group = ManagedGroup::spawn(fixture("worker", root.path()), TASK_ID).unwrap();
        let _stdout = group.take_stdout().unwrap();
        wait_for(|| root.path().join("worker-ready").exists());
        assert!(!root.path().join("work-started").exists());
        group.stop(Duration::from_secs(2)).unwrap();
        assert!(!process_running(group.id() as i32, group.id() as i32).unwrap());
        assert_eq!(group.start(PLAN), Err(ProcessError::Closed));
        group.stop(Duration::from_secs(2)).unwrap();
        root.disable_cleanup(false);
    }
}

#[test]
fn 开始之前控制端断开不会运行任何工具() {
    let mut root = test_root();
    let mut group = ManagedGroup::spawn(fixture("worker", root.path()), TASK_ID).unwrap();
    wait_for(|| root.path().join("worker-ready").exists());
    group.control.take();
    wait_for(|| group.leader_exited().unwrap());
    group.stop(Duration::from_secs(2)).unwrap();
    assert!(!root.path().join("work-started").exists());
    assert!(!root.path().join("heartbeat").exists());
    root.disable_cleanup(false);
}

#[test]
fn 取消及超时均停止真实后代写入后才报告终态() {
    for expired in [false, true] {
        let mut root = test_root();
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
        root.disable_cleanup(false);
    }
}

#[test]
fn 工作进程意外退出仍能停止其后代() {
    let mut root = test_root();
    let mut group = started(root.path());
    group.child.kill().unwrap();
    // 这里故意不 wait，组长 PID 仍被保留；后代不能因组长退出逃过收尾。
    group.stop(Duration::from_secs(2)).unwrap();
    let heartbeat = root.path().join("heartbeat");
    let stopped = fs::read(&heartbeat).unwrap();
    std::thread::sleep(Duration::from_millis(80));
    assert_eq!(fs::read(heartbeat).unwrap(), stopped);
    root.disable_cleanup(false);
}

#[test]
fn 普通命令退出状态仅在确认后代停止后回收() {
    let mut root = test_root();
    let mut group = ManagedGroup::spawn_command(ordinary("early-exit", root.path())).unwrap();
    assert_eq!(group.start(PLAN), Err(ProcessError::Closed));
    group.close_input();
    wait_for(|| {
        root.path()
            .join("heartbeat")
            .metadata()
            .is_ok_and(|m| m.len() > 0)
            && group.leader_exited().unwrap()
    });
    assert!(group.exit_status().is_none());
    group.stop(Duration::from_secs(2)).unwrap();
    assert_eq!(group.exit_status().unwrap().code(), Some(17));
    let stopped = fs::read(root.path().join("heartbeat")).unwrap();
    std::thread::sleep(Duration::from_millis(80));
    assert_eq!(fs::read(root.path().join("heartbeat")).unwrap(), stopped);
    root.disable_cleanup(false);
}

#[test]
fn 普通命令中文空格引号尾反斜杠及环境原样传递() {
    let mut root = test_root();
    let mut spec = ordinary("plain-echo", root.path());
    let values = [
        "",
        "中文 空格",
        "a\"b",
        "尾部\\",
        "空 格\\\\",
        "\\\\\"",
        "&%PATH%|>$HOME",
    ];
    // 测试程序已指定 --exact；额外字面过滤项不匹配任何测试，包含空串也不会启动其他夹具。
    spec.arguments.extend(values.map(Into::into));
    spec.set_env(
        "SHIELD_TEST_COMMAND_DATA",
        std::ffi::OsStr::new("中文 空格=值"),
    );
    let original = std::env::var_os("SHIELD_TEST_COMMAND_DATA");
    let expected = spec
        .arguments
        .iter()
        .map(|v| v.to_str().unwrap().to_owned())
        .collect::<Vec<_>>();
    let mut group = ManagedGroup::spawn_command(spec).unwrap();
    group.close_input();
    let mut output = String::new();
    let mut stdout = group.take_stdout().unwrap();
    wait_for(|| group.leader_exited().unwrap());
    group.stop(Duration::from_secs(2)).unwrap();
    stdout.read_to_string(&mut output).unwrap();
    assert!(output.contains("本次普通命令探针"));
    assert_eq!(group.exit_status().unwrap().code(), Some(0));
    let actual: Vec<String> =
        serde_json::from_slice(&fs::read(root.path().join("arguments")).unwrap()).unwrap();
    assert_eq!(actual, expected);
    assert_eq!(
        fs::read_to_string(root.path().join("environment")).unwrap(),
        "中文 空格=值"
    );
    assert_eq!(
        fs::canonicalize(fs::read_to_string(root.path().join("directory")).unwrap()).unwrap(),
        fs::canonicalize(root.path()).unwrap()
    );
    assert_eq!(std::env::var_os("SHIELD_TEST_COMMAND_DATA"), original);
    root.disable_cleanup(false);
}

#[test]
fn 异常离开父作用域自动停止本次组() {
    let mut root = test_root();
    let pid;
    {
        let group = started(root.path());
        pid = group.id() as i32;
    }
    assert!(!process_running(pid, pid).unwrap());
    let heartbeat = root.path().join("heartbeat");
    let stopped = fs::read(&heartbeat).unwrap();
    std::thread::sleep(Duration::from_millis(80));
    assert_eq!(fs::read(heartbeat).unwrap(), stopped);
    root.disable_cleanup(false);
}

#[test]
fn 父进程异常退出由租约停止仍存活的工作组() {
    let mut root = test_root();
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
    wait_for(|| !process_running(pid, pid).unwrap());
    let child: i32 = fs::read_to_string(root.path().join("child-pid"))
        .unwrap()
        .parse()
        .unwrap();
    wait_for(|| !process_running(child, pid).unwrap());
    let heartbeat = root.path().join("heartbeat");
    let stopped = fs::read(&heartbeat).unwrap();
    std::thread::sleep(Duration::from_millis(80));
    assert_eq!(fs::read(heartbeat).unwrap(), stopped);
    root.disable_cleanup(false);
}

#[test]
fn 父端断开重复开始及停止消息都停止本次工作组() {
    for mode in ["closed", "repeat", "stop"] {
        let mut root = test_root();
        let mut group = started(root.path());
        if mode != "closed" {
            // 直接破坏私有管道仅用于反向测试，不作为产品接口。
            let mut control = group.control.take().unwrap();
            control
                .send(if mode == "repeat" {
                    Message::Start { plan_digest: PLAN }
                } else {
                    Message::Stop {
                        reason: crate::automation::task_lifecycle::StopReason::Cancel,
                    }
                })
                .unwrap();
        } else {
            group.control.take();
        }
        wait_for(|| !process_running(group.id() as i32, group.id() as i32).unwrap());
        group.stop(Duration::from_secs(2)).unwrap();
        let stopped = fs::read(root.path().join("heartbeat")).unwrap();
        std::thread::sleep(Duration::from_millis(80));
        assert_eq!(fs::read(root.path().join("heartbeat")).unwrap(), stopped);
        root.disable_cleanup(false);
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
        "plain-echo" => {
            record(
                &root,
                "arguments",
                &serde_json::to_vec(&std::env::args().skip(1).collect::<Vec<_>>()).unwrap(),
            );
            record(
                &root,
                "environment",
                std::env::var("SHIELD_TEST_COMMAND_DATA")
                    .unwrap()
                    .as_bytes(),
            );
            record(
                &root,
                "directory",
                std::env::current_dir()
                    .unwrap()
                    .to_str()
                    .unwrap()
                    .as_bytes(),
            );
            println!("本次普通命令探针");
        }
        "early-exit" => {
            let child = fixture("writer", &root)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .unwrap();
            record(&root, "child-pid", child.id().to_string().as_bytes());
            wait_for(|| root.join("heartbeat").metadata().is_ok_and(|m| m.len() > 0));
            std::process::exit(17);
        }
        "plain-parent" => {
            let _group = ManagedGroup::spawn_command(ordinary("early-exit", &root)).unwrap();
            wait_for(|| root.join("heartbeat").metadata().is_ok_and(|v| v.len() > 0));
            record(&root, "parent-ready", b"test");
            std::thread::sleep(Duration::from_secs(30));
        }
        "exit-259" => std::process::exit(259),
        "flood" => {
            record(&root, "flood-ready", b"test");
            let _ = std::io::stdout().write_all(&vec![b'x'; 1024 * 1024]);
        }
        #[cfg(windows)]
        "handle-check" => {
            let raw = std::env::var("SHIELD_TEST_EVENT")
                .unwrap()
                .parse::<usize>()
                .unwrap();
            // 若错误继承了测试事件，父端会看到它被设置；不按这个值关闭任何句柄。
            unsafe {
                windows_sys::Win32::System::Threading::SetEvent(raw as *mut _);
            }
        }
        "worker" | "java-worker" | "certificate-worker" | "signing-worker" | "apktool-worker" => {
            let lease = WorkerLease::install(TASK_ID).unwrap();
            record(
                &root,
                "worker-ready",
                std::process::id().to_string().as_bytes(),
            );
            assert_eq!(lease.wait_start(Duration::from_secs(10)).unwrap(), PLAN);
            record(&root, "work-started", b"yes");
            if role == "apktool-worker" {
                apktool_tests::worker(&root);
                return;
            }
            if role == "signing-worker" {
                signing_tests::worker(&root);
                return;
            }
            if role == "certificate-worker" {
                certificate_tests::worker(&root);
                return;
            }
            if role == "java-worker" {
                let context =
                    shield_core::JavaToolContext::new(&root.join("adapter.jar"), &root).unwrap();
                let proxy = root.join(format!("adapter-proxy{}", std::env::consts::EXE_SUFFIX));
                // 专项代理先真实执行助手，再制造未结束的输出及后代；不新增生产测试入口。
                context.verify_runtime(&proxy).unwrap();
                panic!("故障窗口应由任务监督停止，不能自然成功返回");
            }
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
            std::thread::sleep(Duration::from_secs(30));
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
            let end = Instant::now() + Duration::from_secs(30);
            while Instant::now() < end {
                file.write_all(b".").unwrap();
                std::thread::sleep(Duration::from_millis(5));
            }
        }
        "parent" => {
            let _group = started(&root);
            record(&root, "parent-ready", b"yes");
            std::thread::sleep(Duration::from_secs(30));
        }
        _ => panic!("未知夹具角色"),
    }
}

//! 真实解包返回及 aapt2 执行后返回等待监督；不宣称算法内部取消或完整公开任务通过。
use super::*;
use shield_core::diagnostic::{FailureCode, ToolName};
#[path = "native_group_apktool_fixture.rs"]
mod fixture;
use fixture::Harness;

pub(super) fn worker(root: &Path) {
    fixture::worker(root);
}

fn ready(root: &Path, phase: &str) {
    let until = Instant::now() + Duration::from_secs(20);
    while !(root.join("apktool-window-ready").exists()
        && root.join("heartbeat").metadata().is_ok_and(|m| m.len() > 0))
    {
        if root.join("apktool-failed").is_file() {
            let failure: serde_json::Value =
                serde_json::from_slice(&fs::read(root.join("apktool-failed")).unwrap()).unwrap();
            panic!("真实业务已失败而非窗口就绪：{failure}");
        }
        assert!(
            !root.join("apktool-succeeded").exists(),
            "必须实际命中被测阶段"
        );
        assert!(Instant::now() < until, "apktool 原生窗口未就绪");
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(
        fs::read(root.join("apktool-window-ready")).unwrap(),
        phase.as_bytes()
    );
    assert!(root.join("d-java-eof").exists() && root.join("d-real-completed").exists());
    if phase == "native" {
        assert!(root.join("b-java-eof").exists() && root.join("b-real-entered").exists());
        assert_eq!(fs::read(root.join("aapt-completed")).unwrap(), b"0");
        assert!(
            !root.join("b-real-completed").exists(),
            "真实 apktool 仍等待 Native 返回"
        );
    }
}

fn setup(
    harness: &Harness,
    root: &Path,
    phase: &str,
    mode: &str,
    seconds: u64,
) -> (Task, ManagedGroup, impl Read) {
    let mut task = Task::new(Instant::now(), Duration::from_secs(seconds), PLAN, [0; 32]).unwrap();
    let mut group = ManagedGroup::spawn(harness.command(root, phase, mode), TASK_ID).unwrap();
    let stdout = group.take_stdout().unwrap();
    task.isolation_confirmed(Instant::now()).unwrap();
    wait_for(|| root.join("worker-ready").exists());
    task.hello(Instant::now()).unwrap();
    (task, group, stdout)
}

fn stopped(group: &mut ManagedGroup, root: &Path) {
    group.stop(Duration::from_secs(2)).unwrap();
    let before = fs::read(root.join("heartbeat")).unwrap();
    std::thread::sleep(Duration::from_millis(100));
    assert_eq!(fs::read(root.join("heartbeat")).unwrap(), before);
}

#[test]
#[ignore = "需正式 JAR、自有 APK 和 Java 8／17；真实解包及 aapt2 返回等待的整组监督"]
fn 解包及原生重建取消超时断开停止后代且不发布() {
    let harness = Harness::new();
    for phase in ["decode", "native"] {
        for mode in ["cancel", "timeout", "disconnect"] {
            let mut case = harness.case();
            let root = case.root.path();
            let (mut task, mut group, _stdout) = setup(
                &harness,
                root,
                phase,
                "hold",
                if mode == "timeout" { 8 } else { 30 },
            );
            task.start(Instant::now()).unwrap();
            group.start(PLAN).unwrap();
            ready(root, phase);
            let pid = fs::read_to_string(root.join("native-proxy-pid"))
                .unwrap()
                .parse()
                .unwrap();
            assert!(process_running(pid, group.id() as i32).unwrap());
            harness.unchanged(&case);
            harness.retained(root);
            match mode {
                "cancel" => task.cancel(Instant::now()).unwrap(),
                "disconnect" => {
                    group.control.take();
                    wait_for(|| group.leader_exited().unwrap());
                    task.channel_closed(Instant::now()).unwrap();
                }
                _ => {
                    while task.stop_reason().is_none() {
                        let _ = task.checkpoint(Instant::now());
                        std::thread::sleep(Duration::from_millis(5));
                    }
                }
            }
            assert!(!task.can_cleanup() && task.take_outcome().is_none());
            stopped(&mut group, root);
            harness.unchanged(&case);
            harness.retained(root);
            task.tree_stopped(Instant::now()).unwrap();
            let result = task.take_outcome().unwrap();
            assert_eq!(
                result.output,
                crate::automation::task_lifecycle::Output::NotPublished
            );
            assert_eq!(
                result.status,
                match mode {
                    "cancel" => Status::Cancelled,
                    "timeout" => Status::TimedOut,
                    _ => Status::Failed(crate::automation::task_lifecycle::Failure::UnexpectedExit),
                }
            );
            assert!(task.can_cleanup());
            case.root.disable_cleanup(false);
            println!("apktool 停止结果：窗口={phase}，模式={mode}，真实阶段及原生整组停止通过，原包／输出／资料不变");
        }
    }
}

#[test]
#[ignore = "需实际 aapt2；开始前取消、停止未知、双流排空和真实调用后的故障"]
fn 原生重建故障及未知保持材料且双流持续排空() {
    let harness = Harness::new();
    for mode in ["before", "unknown", "flood-hold", "exit", "flood"] {
        let mut case = harness.case();
        let root = case.root.path();
        let (mut task, mut group, _stdout) = setup(
            &harness,
            root,
            "native",
            if mode == "exit" || mode == "flood" {
                mode
            } else {
                "flood-hold"
            },
            30,
        );
        if mode != "before" {
            task.start(Instant::now()).unwrap();
            group.start(PLAN).unwrap();
            ready(root, "native");
            if mode != "exit" {
                wait_for(|| root.join("apktool-flood-written").exists());
            }
        }
        if mode == "exit" || mode == "flood" {
            wait_for(|| root.join("apktool-failed").exists());
            let result: serde_json::Value =
                serde_json::from_slice(&fs::read(root.join("apktool-failed")).unwrap()).unwrap();
            assert_eq!(
                result["tool"],
                serde_json::to_value(ToolName::Apktool).unwrap()
            );
            assert_eq!(
                result["code"],
                serde_json::to_value(FailureCode::ApkRepackFailed).unwrap()
            );
            assert!(result["exit"].as_i64().is_some_and(|code| code > 0));
            if mode == "flood" {
                assert!(root.join("java-flood-written").is_file());
                assert_eq!(result["truncated"], true);
            }
            task.worker_failed(Instant::now(), FailureCode::ApkRepackFailed)
                .unwrap();
            let size = fs::metadata(root.join("heartbeat")).unwrap().len();
            std::thread::sleep(Duration::from_millis(40));
            assert!(fs::metadata(root.join("heartbeat")).unwrap().len() > size);
        } else {
            task.cancel(Instant::now()).unwrap();
            if mode == "unknown" {
                task.stop_unconfirmed(Instant::now()).unwrap();
                assert_eq!(task.take_outcome().unwrap().status, Status::Unknown);
                assert!(task.start(Instant::now()).is_err());
                assert!(task.begin_publish(Instant::now()).is_err());
            }
        }
        assert!(!task.can_cleanup());
        if mode == "before" {
            group.stop(Duration::from_secs(2)).unwrap();
            assert!(!root.join("d-java-eof").exists());
        } else {
            harness.retained(root);
            stopped(&mut group, root);
            harness.retained(root);
        }
        harness.unchanged(&case);
        if mode == "unknown" {
            assert!(!task.can_cleanup());
        } else {
            task.tree_stopped(Instant::now()).unwrap();
            let result = task.take_outcome().unwrap();
            assert_eq!(
                result.output,
                crate::automation::task_lifecycle::Output::NotPublished
            );
            assert_eq!(
                result.status,
                if mode == "exit" || mode == "flood" {
                    Status::Failed(crate::automation::task_lifecycle::Failure::Worker(
                        FailureCode::ApkRepackFailed,
                    ))
                } else {
                    Status::Cancelled
                }
            );
            assert!(task.can_cleanup());
        }
        case.root.disable_cleanup(false);
        println!("apktool 故障保留结果：模式={mode}，原生停止前不清理、不发布，原包和共享资料不变");
    }
}

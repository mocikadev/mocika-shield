//! 真实签名／验签的 JVM 与返回等待窗口；不替代算法内部协作取消或公开入口绑定。
use super::*;
use shield_core::diagnostic::{FailureCode, ToolName};
#[path = "native_group_signing_fixture.rs"]
mod fixture;
use super::certificate_fixture::{Case, Denied};
use fixture::Harness;

pub(super) fn worker(root: &Path) {
    fixture::run_worker(root);
}

fn ready(root: &Path, phase: &str) {
    let until = Instant::now() + Duration::from_secs(20);
    let operation = phase.split_once('-').unwrap().0;
    while !(root.join(format!("{phase}-ready")).exists()
        && root.join(format!("{operation}-java-eof")).exists()
        && root.join("signing-stdin-empty").exists()
        && root
            .join("heartbeat")
            .metadata()
            .is_ok_and(|meta| meta.len() > 0))
    {
        assert!(
            !root.join("signing-succeeded").exists(),
            "故障未命中真实签名／验签窗口"
        );
        assert!(Instant::now() < until, "真实签名／验签窗口未就绪");
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(root.join(format!("{operation}-request-entered")).exists());
    assert!(root.join(format!("{operation}-real-entered")).exists());
}

fn setup(
    harness: &Harness,
    case: &Case,
    phase: &str,
    mode: &str,
    seconds: u64,
) -> (Task, ManagedGroup, impl Read) {
    let root = case.root.path();
    let mut task = harness.task(seconds);
    let mut group = ManagedGroup::spawn(harness.command(root, phase, mode), TASK_ID).unwrap();
    let stdout = group.take_stdout().unwrap();
    task.isolation_confirmed(Instant::now()).unwrap();
    wait_for(|| root.join("worker-ready").exists());
    task.hello(Instant::now()).unwrap();
    (task, group, stdout)
}

fn stopped(root: &Path) {
    let before = fs::read(root.join("heartbeat")).unwrap();
    std::thread::sleep(Duration::from_millis(100));
    assert_eq!(fs::read(root.join("heartbeat")).unwrap(), before);
}

#[test]
#[ignore = "需正式 JAR、自有真实 APK、SDK 校验及显式 Java 8／17，运行原生签名窗口矩阵"]
fn 真实签名验签各窗口取消超时断开均停止后代且不发布() {
    let harness = Harness::new();
    for phase in ["sign-jar", "sign-output", "verify-jar", "verify-output"] {
        for mode in ["cancel", "timeout", "disconnect"] {
            let mut case = harness.case();
            let root = case.root.path();
            let (mut task, mut group, _stdout) = setup(
                &harness,
                &case,
                phase,
                "hold",
                if mode == "timeout" { 8 } else { 30 },
            );
            task.start(Instant::now()).unwrap();
            group.start(PLAN).unwrap();
            ready(root, phase);
            let operation = phase.split_once('-').unwrap().0;
            assert_eq!(
                fs::read_to_string(root.join(format!("{operation}-status"))).unwrap(),
                "0"
            );
            assert!(root.join(format!("{operation}-real-completed")).exists());
            let pid: i32 = fs::read_to_string(root.join(format!("{operation}-java-pid")))
                .unwrap()
                .parse()
                .unwrap();
            assert_eq!(
                process_running(pid, group.id() as i32).unwrap(),
                phase.ends_with("-jar"),
                "必须区分真实 JVM 仍存活与 JVM 已退出但结果未交付，不能只看代理存活"
            );
            harness.verify_candidate(root);
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
            harness.unchanged(&case);
            assert!(root.join("work/staged.apk").is_file());
            group.stop(Duration::from_secs(2)).unwrap();
            stopped(root);
            harness.unchanged(&case);
            harness.retained(root);
            task.tree_stopped(Instant::now()).unwrap();
            let outcome = task.take_outcome().unwrap();
            assert_eq!(
                outcome.output,
                crate::automation::task_lifecycle::Output::NotPublished
            );
            assert_eq!(
                outcome.status,
                match mode {
                    "cancel" => Status::Cancelled,
                    "timeout" => Status::TimedOut,
                    _ => Status::Failed(crate::automation::task_lifecycle::Failure::UnexpectedExit),
                }
            );
            assert!(task.can_cleanup());
            case.root.disable_cleanup(false);
            println!("签名监督结果：窗口={phase}，模式={mode}，真实签名及独立验签通过，整组停止后才允许清理，原包／最终输出／资料不变");
        }
    }
}

#[test]
#[ignore = "需真实工具及原生后代，验证签名前取消、停止未知和双流背压等待"]
fn 真实签名开始前取消及停止未知不清理且双流持续排空() {
    let harness = Harness::new();
    for mode in ["before", "unknown", "flood-hold"] {
        let mut case = harness.case();
        let root = case.root.path();
        let (mut task, mut group, _stdout) =
            setup(&harness, &case, "sign-output", "flood-hold", 30);
        if mode != "before" {
            task.start(Instant::now()).unwrap();
            group.start(PLAN).unwrap();
            ready(root, "sign-output");
            wait_for(|| root.join("flood-written").exists());
            harness.verify_candidate(root);
        }
        task.cancel(Instant::now()).unwrap();
        if mode == "unknown" {
            task.stop_unconfirmed(Instant::now()).unwrap();
            assert_eq!(task.take_outcome().unwrap().status, Status::Unknown);
            assert!(
                task.start(Instant::now()).is_err() && task.begin_publish(Instant::now()).is_err()
            );
        }
        assert!(!task.can_cleanup());
        harness.unchanged(&case);
        group.stop(Duration::from_secs(2)).unwrap();
        if mode == "before" {
            assert!(!root.join("work-started").exists());
            assert!(!root.join("sign-request-entered").exists());
            assert!(!root.join("work/staged.apk").exists());
        } else {
            stopped(root);
            harness.retained(root);
            assert!(root.join("work/staged.apk").is_file());
        }
        if mode != "unknown" {
            task.tree_stopped(Instant::now()).unwrap();
            assert_eq!(task.take_outcome().unwrap().status, Status::Cancelled);
        } else {
            assert!(!task.can_cleanup(), "真实组停不把 Unknown 自动改成可清理");
        }
        harness.unchanged(&case);
        case.root.disable_cleanup(false);
        println!("签名保留结果：模式={mode}，开始前未执行／未知禁复用／双流各 1 MiB 排空的对应断言通过，仅在原生停止证明后由测试所有者清理");
    }
}

#[test]
#[ignore = "需真实签名工具；验证非零、双流超量、非法编码和权限拒绝后仍须整组停止"]
fn 真实签名验签工具故障输出异常权限拒绝均不提交() {
    let harness = Harness::new();
    for (phase, mode) in [
        ("sign-output", "exit"),
        ("verify-output", "exit"),
        ("sign-output", "flood"),
        ("verify-output", "flood"),
        ("verify-output", "invalid-utf8"),
        ("sign-output", "permission"),
        ("verify-output", "permission"),
    ] {
        let mut case = harness.case();
        let root = case.root.path();
        let (mut task, mut group, _stdout) = setup(&harness, &case, phase, mode, 30);
        task.start(Instant::now()).unwrap();
        group.start(PLAN).unwrap();
        let permission = if mode == "permission" {
            wait_for(|| root.join("permission-ready").exists());
            let path = if phase.starts_with("sign-") {
                root.join("app/keystores/existing.jks")
            } else {
                root.join("work/staged.apk")
            };
            let guard = Denied::new(&path);
            record(root, "permission-release", b"yes");
            Some(guard)
        } else {
            None
        };
        ready(root, phase);
        wait_for(|| root.join("worker-failed").exists());
        let operation = phase.split_once('-').unwrap().0;
        let actual: i32 = fs::read_to_string(root.join(format!("{operation}-jvm-exit")))
            .unwrap()
            .parse()
            .unwrap();
        assert_eq!(
            actual == 0,
            mode != "permission",
            "权限必须实际让原工具失败，其他注入前须真实成功"
        );
        let result: serde_json::Value =
            serde_json::from_slice(&fs::read(root.join("worker-failed")).unwrap()).unwrap();
        let code = if mode == "invalid-utf8" {
            FailureCode::ToolOutputEncodingInvalid
        } else {
            FailureCode::ToolProcessFailed
        };
        assert_eq!(result["code"], serde_json::to_value(code).unwrap());
        assert_eq!(
            result["tool"],
            serde_json::to_value(ToolName::Apksigner).unwrap()
        );
        if mode == "exit" || (mode == "flood" && operation == "sign") {
            assert_eq!(result["exit"], 41);
        }
        if mode == "invalid-utf8" || (mode == "flood" && operation == "verify") {
            assert_eq!(result["exit"], 0);
        }
        if mode == "flood" {
            assert!(root.join("flood-written").exists());
        }
        #[cfg(windows)]
        {
            let flags: serde_json::Value =
                serde_json::from_slice(&fs::read(root.join("proxy-output-flags")).unwrap())
                    .unwrap();
            assert_eq!(flags["after"], false);
        }
        task.worker_failed(Instant::now(), code).unwrap();
        assert!(!task.can_cleanup());
        harness.artifacts_unchanged(root);
        harness.retained(root);
        let before = fs::metadata(root.join("heartbeat")).unwrap().len();
        std::thread::sleep(Duration::from_millis(40));
        assert!(
            fs::metadata(root.join("heartbeat")).unwrap().len() > before,
            "回执或 Java 退出不能作为后代已停证明"
        );
        group.stop(Duration::from_secs(2)).unwrap();
        stopped(root);
        if let Some(permission) = permission {
            permission.restore();
        }
        harness.unchanged(&case);
        harness.retained(root);
        task.tree_stopped(Instant::now()).unwrap();
        assert_eq!(
            task.take_outcome().unwrap().status,
            Status::Failed(crate::automation::task_lifecycle::Failure::Worker(code))
        );
        assert!(task.can_cleanup());
        case.root.disable_cleanup(false);
        println!("签名故障结果：窗口={phase}，模式={mode}，实际工具状态及分类匹配，后代原生停止前不清理，原件与资料不变");
    }
}

//! 真实新建用例在各阶段边界的故障与停止监督；不证明默认入口或提交区间已接线。
use super::certificate_fixture as fixture;
use super::*;
use fixture::Harness;
use shield_core::diagnostic::FailureCode;

pub(super) fn worker(root: &Path) {
    fixture::run_worker(root);
}

fn wait_ready(root: &Path, phase: &str) {
    let end = Instant::now() + Duration::from_secs(20);
    while !(root.join(format!("{phase}-completed")).exists()
        && root.join("certificate-stdin-empty").exists()
        && root
            .join("heartbeat")
            .metadata()
            .is_ok_and(|meta| meta.len() > 0))
    {
        assert!(
            !root.join("worker-failed").exists(),
            "新建阶段未成功进入停止窗口"
        );
        assert!(
            !root.join("worker-succeeded").exists(),
            "未实际注入故障窗口"
        );
        assert!(Instant::now() < end, "新建阶段未在期限内就绪");
        std::thread::sleep(Duration::from_millis(5));
    }
}
fn stopped(root: &Path) {
    let before = fs::read(root.join("heartbeat")).unwrap();
    std::thread::sleep(Duration::from_millis(100));
    assert_eq!(fs::read(root.join("heartbeat")).unwrap(), before);
}

#[test]
#[ignore = "需正式 JAR 与显式 Java 8／17，真实新建三阶段及原生整组停止"]
fn 新建各阶段取消超时及断开停止后代且不入库() {
    let harness = Harness::new();
    for phase in ["prepare", "generate", "finish"] {
        for mode in ["cancel", "timeout", "disconnect"] {
            let mut case = harness.case();
            let root = case.root.path();
            let mut task = Task::new(
                Instant::now(),
                Duration::from_secs(if mode == "timeout" { 8 } else { 30 }),
                PLAN,
                [0; 32],
            )
            .unwrap();
            let mut group =
                ManagedGroup::spawn(harness.command(root, phase, "hold"), TASK_ID).unwrap();
            let _stdout = group.take_stdout().unwrap();
            task.isolation_confirmed(Instant::now()).unwrap();
            wait_for(|| root.join("worker-ready").exists());
            task.hello(Instant::now()).unwrap();
            task.start(Instant::now()).unwrap();
            group.start(PLAN).unwrap();
            wait_ready(root, phase);
            assert_eq!(
                fs::read_to_string(root.join(format!("{phase}-status"))).unwrap(),
                "0"
            );
            case.retained(phase);
            match mode {
                "cancel" => task.cancel(Instant::now()).unwrap(),
                "disconnect" => {
                    group.control.take();
                    // 先观察租约撤销导致工作端退出，再进行独立整组复核；不与租约同时发起终止。
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
            assert!(!task.can_cleanup());
            assert!(task.take_outcome().is_none());
            case.retained(phase);
            group.stop(Duration::from_secs(2)).unwrap();
            stopped(root);
            case.retained(phase);
            task.tree_stopped(Instant::now()).unwrap();
            let outcome = task.take_outcome().unwrap();
            assert_eq!(
                outcome.output,
                crate::automation::task_lifecycle::Output::NotPublished
            );
            match mode {
                "cancel" => assert_eq!(outcome.status, Status::Cancelled),
                "timeout" => assert_eq!(outcome.status, Status::TimedOut),
                _ => assert_eq!(
                    outcome.status,
                    Status::Failed(crate::automation::task_lifecycle::Failure::UnexpectedExit)
                ),
            }
            assert!(task.can_cleanup());
            case.root.disable_cleanup(false);
            println!("新建监督结果：阶段={phase}，模式={mode}，真实阶段已命中，后代已停止，资料与默认项不变");
        }
    }
}

#[test]
#[ignore = "需正式 JAR 与 JDK，验证开始前不执行及停止未知不清理不复用"]
fn 新建开始前取消不运行工具且停止未知保留材料() {
    let harness = Harness::new();
    for before in [true, false] {
        let mut case = harness.case();
        let root = case.root.path();
        let mut task = Task::new(Instant::now(), Duration::from_secs(30), PLAN, [0; 32]).unwrap();
        let mut group =
            ManagedGroup::spawn(harness.command(root, "finish", "hold"), TASK_ID).unwrap();
        let _stdout = group.take_stdout().unwrap();
        task.isolation_confirmed(Instant::now()).unwrap();
        wait_for(|| root.join("worker-ready").exists());
        task.hello(Instant::now()).unwrap();
        if !before {
            task.start(Instant::now()).unwrap();
            group.start(PLAN).unwrap();
            wait_ready(root, "finish");
        }
        task.cancel(Instant::now()).unwrap();
        if !before {
            task.stop_unconfirmed(Instant::now()).unwrap();
            assert_eq!(task.take_outcome().unwrap().status, Status::Unknown);
            assert!(!task.can_cleanup());
            assert!(task.start(Instant::now()).is_err());
            assert!(task.begin_publish(Instant::now()).is_err());
            case.retained("finish");
        }
        group.stop(Duration::from_secs(2)).unwrap();
        if before {
            assert!(!root.join("work-started").exists());
            assert!(!root.join("prepare-entered").exists());
            task.tree_stopped(Instant::now()).unwrap();
            assert_eq!(task.take_outcome().unwrap().status, Status::Cancelled);
        } else {
            stopped(root);
            assert!(!task.can_cleanup());
        }
        case.unchanged();
        // 测试所有者在真实整组停止后手工回收本次夹具；不冒充 Unknown 已允许产品自动清理。
        case.root.disable_cleanup(false);
    }
    println!(
        "新建未知结果：开始前取消未运行，停止未知保留材料且禁止复用／发布，手工确认停止后回收夹具"
    );
}

#[test]
#[ignore = "需正式 JAR 与 JDK，仅本次隔离材料的工具失败、结果及权限注入"]
fn 新建工具失败异常输出及权限拒绝不入库且外层停止后代() {
    let harness = Harness::new();
    for (phase, mode) in [
        ("prepare", "exit"),
        ("generate", "exit"),
        ("finish", "exit"),
        ("finish", "malformed"),
        ("finish", "flood"),
        ("prepare", "permission"),
        ("generate", "permission"),
        ("finish", "permission"),
    ] {
        let mut case = harness.case();
        let root = case.root.path();
        let mut task = Task::new(Instant::now(), Duration::from_secs(30), PLAN, [0; 32]).unwrap();
        let mut group = ManagedGroup::spawn(harness.command(root, phase, mode), TASK_ID).unwrap();
        let _stdout = group.take_stdout().unwrap();
        task.isolation_confirmed(Instant::now()).unwrap();
        wait_for(|| root.join("worker-ready").exists());
        task.hello(Instant::now()).unwrap();
        task.start(Instant::now()).unwrap();
        group.start(PLAN).unwrap();
        let permission = if mode == "permission" {
            wait_for(|| root.join("permission-ready").exists());
            let guard = fixture::Denied::new(&case.material(phase));
            record(root, "permission-release", b"yes");
            Some(guard)
        } else {
            None
        };
        let end = Instant::now() + Duration::from_secs(20);
        while !root.join("worker-failed").exists() {
            assert!(!root.join("worker-succeeded").exists());
            assert!(Instant::now() < end);
            std::thread::sleep(Duration::from_millis(5));
        }
        wait_ready(root, phase);
        #[cfg(windows)]
        {
            let flags: serde_json::Value =
                serde_json::from_slice(&fs::read(root.join("proxy-output-flags")).unwrap())
                    .unwrap();
            assert_eq!(flags["after"], false);
            println!(
                "新建代理输出继承：阶段={phase}，故障={mode}，原可继承={}，现不可继承",
                flags["before"]
            );
        }
        assert_eq!(
            fs::read_to_string(root.join(format!("{phase}-status"))).unwrap() == "0",
            mode != "permission"
        );
        let result: serde_json::Value =
            serde_json::from_slice(&fs::read(root.join("worker-failed")).unwrap()).unwrap();
        let code = if mode == "permission" && phase != "generate" {
            FailureCode::PermissionDenied
        } else {
            FailureCode::ToolProcessFailed
        };
        assert_eq!(result["code"], serde_json::to_value(code).unwrap());
        if phase == "generate" && mode == "exit" {
            assert_eq!(result["exit"], 41);
        }
        task.worker_failed(Instant::now(), code).unwrap();
        assert!(!task.can_cleanup());
        case.unchanged();
        let heartbeat = fs::metadata(root.join("heartbeat")).unwrap().len();
        std::thread::sleep(Duration::from_millis(40));
        assert!(
            fs::metadata(root.join("heartbeat")).unwrap().len() > heartbeat,
            "核心单进程退出不能冒充后代已停止"
        );
        group.stop(Duration::from_secs(2)).unwrap();
        stopped(root);
        if let Some(permission) = permission {
            permission.restore();
        }
        case.retained(phase);
        task.tree_stopped(Instant::now()).unwrap();
        assert_eq!(
            task.take_outcome().unwrap().status,
            Status::Failed(crate::automation::task_lifecycle::Failure::Worker(code))
        );
        assert!(task.can_cleanup());
        case.root.disable_cleanup(false);
        println!("新建故障结果：阶段={phase}，故障={mode}，分类正确、资料不变，外层确认后代停止");
    }
}

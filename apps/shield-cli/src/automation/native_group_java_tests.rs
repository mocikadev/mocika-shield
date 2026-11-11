//! 正式只读输入与原任务租约／停止监督的交叉验证，不接入证书或真实资料。
use super::*;

#[test]
#[ignore = "需预构建正式 Java 适配 JAR 及显式选择 JDK，运行原生整组停止专项"]
fn 正式助手的只读输入不混用租约且取消超时断开停止后代() {
    let build = tempfile::tempdir().unwrap();
    let source = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/agent-signing/AdapterLifecycle.rs");
    let proxy = build
        .path()
        .join(format!("adapter-proxy{}", std::env::consts::EXE_SUFFIX));
    assert!(
        shield_core::utils::no_window_command("rustc")
            .arg(&source)
            .args(["--edition=2021", "-o"])
            .arg(&proxy)
            .output()
            .unwrap()
            .status
            .success(),
        "原生助手监督夹具编译失败"
    );
    let jar = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tools/java-adapter/build/java-adapter.jar");
    let java = shield_core::utils::find_java().unwrap();
    for mode in ["before", "cancel", "timeout", "disconnect"] {
        let mut root = test_root();
        {
            // 只重建本测试刚分配的空目录，不修改任何用户目录的权限。
            fs::remove_dir(root.path()).unwrap();
            #[cfg(windows)]
            shield_core::windows_private::create_directory(root.path()).unwrap();
            #[cfg(unix)]
            {
                use std::os::unix::fs::DirBuilderExt;
                fs::DirBuilder::new()
                    .mode(0o700)
                    .create(root.path())
                    .unwrap();
            }
        }
        fs::copy(&proxy, root.path().join(proxy.file_name().unwrap())).unwrap();
        fs::copy(&jar, root.path().join("adapter.jar")).unwrap();
        let mut command = fixture("java-worker", root.path());
        command.env("MOCIKA_TEST_ADAPTER_JAVA", &java);
        let now = Instant::now();
        let mut task = Task::new(
            now,
            Duration::from_secs(if mode == "timeout" { 2 } else { 10 }),
            PLAN,
            [0; 32],
        )
        .unwrap();
        let mut group = ManagedGroup::spawn(command, TASK_ID).unwrap();
        // 取走工作端测试框架 stdout，避免无关输出占满管道。
        let _stdout = group.take_stdout().unwrap();
        task.isolation_confirmed(Instant::now()).unwrap();
        wait_for(|| root.path().join("worker-ready").exists());
        task.hello(Instant::now()).unwrap();
        if mode == "before" {
            task.cancel(Instant::now()).unwrap();
        } else {
            task.start(Instant::now()).unwrap();
            group.start(PLAN).unwrap();
            wait_for(|| {
                root.path().join("adapter-complete").exists()
                    && root.path().join("adapter-stdin-empty").exists()
                    && root
                        .path()
                        .join("heartbeat")
                        .metadata()
                        .is_ok_and(|m| m.len() > 0)
            });
            if mode == "timeout" {
                while task.stop_reason().is_none() {
                    let _ = task.checkpoint(Instant::now());
                    std::thread::sleep(Duration::from_millis(5));
                }
            } else if mode == "disconnect" {
                group.control.take();
            } else {
                task.cancel(Instant::now()).unwrap();
            }
        }
        assert!(!task.can_cleanup());
        assert!(root.path().join("adapter.jar").is_file());
        group.stop(Duration::from_secs(2)).unwrap();
        if mode == "before" {
            assert!(!root.path().join("work-started").exists());
            assert!(!root.path().join("adapter-complete").exists());
            assert_eq!(fs::read_dir(root.path()).unwrap().count(), 3);
        } else {
            let heartbeat = root.path().join("heartbeat");
            let stopped = fs::read(&heartbeat).unwrap();
            std::thread::sleep(Duration::from_millis(80));
            assert_eq!(fs::read(heartbeat).unwrap(), stopped);
            assert!(fs::read_dir(root.path()).unwrap().any(|entry| entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with("java-tool-")));
        }
        if mode != "disconnect" {
            task.tree_stopped(Instant::now()).unwrap();
            assert_eq!(
                task.take_outcome().unwrap().status,
                if mode == "timeout" {
                    Status::TimedOut
                } else {
                    Status::Cancelled
                }
            );
            assert!(task.can_cleanup());
        }
        root.disable_cleanup(false);
        println!("正式助手监督结果：模式={mode}，停止前保留材料，停止后后代无继续写入");
    }
}

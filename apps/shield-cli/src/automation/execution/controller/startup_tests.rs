//! 确定性触发进程启动前中断，验证显式目录清理，不依赖整机三秒内达到工具。
use super::*;

#[test]
fn 启动前取消和准备到期均回传真实清理结果() {
    for expire in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let request = Request {
            config: Some(root.path().join("shield-cli.toml")),
            shared: None,
            sign: None,
            input: root.path().join("input.apk"),
            output: root.path().join("output.apk"),
            runtime_profile: Profile::Standard,
            exclude_abis: vec![],
            timeout_seconds: 30,
        };
        let cancel = AtomicBool::new(false);
        let called = std::cell::Cell::new(false);
        let deadline = Instant::now() + Duration::from_secs(if expire { 1 } else { 30 });
        let mut launcher = Launcher::new(|| {
            called.set(true);
            if expire {
                std::thread::sleep(
                    deadline.saturating_duration_since(Instant::now()) + Duration::from_millis(1),
                );
            } else {
                cancel.store(true, Ordering::SeqCst);
            }
            shield_core::utils::no_window_command(std::env::current_exe().unwrap())
        });
        let failed = planning(&request, deadline, &cancel, &mut launcher)
            .err()
            .unwrap();
        assert!(called.get(), "必须覆盖创建目录后、启动工作进程前的中断");
        assert_eq!(
            failed.code,
            if expire {
                "TASK_TIMEOUT"
            } else {
                "TASK_CANCELLED"
            }
        );
        assert_eq!(failed.cleanup_complete, Some(true));
        assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);
    }
}

#[test]
fn 新适配私有契约与旧平台版本明确分离() {
    let mut request: Request = serde_json::from_value(serde_json::json!({
        "config":"project.toml","input":"input.apk","output":"output.apk",
        "runtime_profile":"standard","exclude_abis":[],"timeout_seconds":30
    }))
    .unwrap();
    assert_eq!(
        request.bootstrap_version(),
        if cfg!(windows) { 5 } else { 2 }
    );
    assert_eq!(request.proof_version(), if cfg!(windows) { 4 } else { 1 });
    request.sign = Some(crate::automation::sign_plan::Request {
        app_home: None,
        certificate_id: None,
        binding: None,
    });
    assert_eq!(
        request.bootstrap_version(),
        if cfg!(windows) { 6 } else { 4 }
    );
    assert_eq!(request.proof_version(), if cfg!(windows) { 5 } else { 3 });
}

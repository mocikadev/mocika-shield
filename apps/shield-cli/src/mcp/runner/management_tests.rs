use super::*;
use std::{fs, os::unix::fs::PermissionsExt, sync::Mutex, time::Duration};

fn fixture(root: &std::path::Path, mode: &str) -> Arc<Policy> {
    fs::create_dir(root.join("input")).unwrap();
    fs::create_dir(root.join("output")).unwrap();
    let settings = json!({"schema_version":1,"tool_version":env!("CARGO_PKG_VERSION"),"operation":"config","type":"result","status":"ready","data":{"protect_defaults":{"runtime_mode":"standard","environment_policy":"compatible","sign_after_protect":null,"certificate_id":"hidden"}}});
    let error = json!({"schema_version":1,"tool_version":env!("CARGO_PKG_VERSION"),"operation":"config","type":"error","code":"CONFIG_OPERATION_FAILED","change_state":"unconfirmed","message":"private-password"});
    let action = match mode {
        "invalid" => "printf 'private-invalid-output\\n'".into(),
        "timeout" => "/bin/sleep 10".into(),
        _ => format!("printf '%s\\n' '{error}'; exit 1"),
    };
    let script=format!("#!/bin/sh\ncase \" $* \" in *' config show '*) printf '%s\\n' '{settings}';; *) printf x >> '{}'; {action};; esac\n",root.join("writes").display());
    let cli = root.join("shield");
    fs::write(&cli, script).unwrap();
    fs::set_permissions(&cli, fs::Permissions::from_mode(0o700)).unwrap();
    Arc::new(
        Policy::new(&root.join("input"), &root.join("output"), None, &cli, 1)
            .unwrap()
            .with_shared(Some(&root.join("home")), vec!["allowed".into()])
            .unwrap(),
    )
}
fn runner(policy: Arc<Policy>, session: session::Shared) -> Runner {
    let (progress, _) = tokio::sync::mpsc::channel(16);
    Runner {
        policy,
        session,
        cancelled: CancellationToken::new(),
        shutdown: CancellationToken::new(),
        progress,
        deadline: tokio::time::Instant::now() + Duration::from_secs(1),
    }
}
#[tokio::test]
async fn 异常回执和超时保留未知修改并锁写但允许查询() {
    for mode in ["invalid", "error", "timeout"] {
        let root = tempfile::tempdir().unwrap();
        let policy = fixture(root.path(), mode);
        let session = Arc::new(Mutex::new(session::Session::default()));
        let value = runner(policy.clone(), session.clone())
            .run(
                "shield_update_settings",
                json!({"environment_policy":"strict"}),
            )
            .await;
        assert_eq!(value["status"], "unknown");
        assert_eq!(value["change_state"], "unconfirmed");
        assert_eq!(value["session_execution_blocked"], true);
        assert!(!value.to_string().contains("private-"));
        let next = runner(policy.clone(), session.clone())
            .run(
                "shield_update_settings",
                json!({"environment_policy":"strict"}),
            )
            .await;
        assert_eq!(next["code"], "SESSION_RECOVERY_REQUIRED");
        assert_eq!(fs::read(root.path().join("writes")).unwrap(), b"x");
        let read = runner(policy, session.clone())
            .run("shield_settings", json!({}))
            .await;
        assert_eq!(read["status"], "ready");
        assert_eq!(
            read["data"]["protect_defaults"]["certificate_scope"],
            "outside_scope"
        );
        assert!(!read.to_string().contains("hidden"));
        assert!(session::lock(&session).start_sign().is_err());
    }
}

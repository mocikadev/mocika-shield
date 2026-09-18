use super::*;
use crate::app_config::{AppConfig, AppConfigState};
use crate::application_identity::ApplicationIdentity;

#[test]
fn application_过期清理后取消失败仍禁止旧任务和重试且重开不复活() {
    removed_reference_fails_closed(false);
}

#[test]
fn application_容量淘汰后取消失败仍禁止旧任务和重试且重开不复活() {
    removed_reference_fails_closed(true);
}

fn removed_reference_fails_closed(capacity: bool) {
    use super::inspection::FileIdentity;
    use crate::task_manager::{TaskKind, TaskStatus};
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("input.apk");
    std::fs::write(&path, "测试输入").unwrap();
    let config = AppConfigState::new(dir.path().join("config.toml"), AppConfig::default());
    let state = SharingState::default();
    let mut store = state.inspections.lock().unwrap();
    let id = store.insert(
        path.clone(),
        FileIdentity::read(&path).unwrap(),
        identity(),
        100,
        |package| state.suppress(&config, package),
    );
    let choice = SharingChoice {
        inspection_id: id.clone(),
        enabled: true,
    };
    for task_id in ["取消后完成", "重开后完成"] {
        let snapshot = store
            .freeze(&state, &config, &path, Some(&choice), "sign", 101)
            .unwrap();
        store.tasks.insert(task_id.into(), snapshot);
    }
    let queued_at = if capacity { 110 } else { 350 };
    state.enqueue(
        &config,
        identity(),
        0,
        "sign",
        "2026-09-18".into(),
        queued_at,
    );
    state.enqueue(
        &config,
        identity(),
        0,
        "sign",
        "2026-09-18".into(),
        queued_at,
    );
    let retry = state.next(&config, queued_at).unwrap();
    state.sent(&retry, true, queued_at);
    let in_flight = state.next(&config, queued_at).unwrap();
    let removed_at = if capacity { 200 } else { 401 };
    let mut unrelated = identity();
    unrelated.package_name = "com.example.other".into();
    let mut newest = String::new();
    for _ in 0..if capacity { 64 } else { 1 } {
        newest = store.insert(
            path.clone(),
            FileIdentity::read(&path).unwrap(),
            unrelated.clone(),
            removed_at,
            |package| state.suppress(&config, package),
        );
    }
    // 必须是真的删除，而不只是 created 已过期：负向查找已经拿不到绑定。
    assert!(store.preference_package(&id, removed_at, false).is_none());
    assert!(store.package(&newest, removed_at).is_some());
    drop(store);
    assert!(state
        .authorization(&config, "com.example.Shield_App")
        .is_none());
    assert!(state.authorization(&config, "com.example.other").is_some());
    assert_eq!(state.pending_count(), 0);
    state.complete(
        &config,
        "取消后完成",
        TaskKind::Sign,
        TaskStatus::Succeeded,
        false,
        removed_at,
    );
    assert!(state.next(&config, removed_at).is_none());
    // 再次检查原包也必须沿用会话关闭；清理不能擅自持久化永久退出。
    let mut store = state.inspections.lock().unwrap();
    let renewed = store.insert(
        path.clone(),
        FileIdentity::read(&path).unwrap(),
        identity(),
        removed_at + 1,
        |package| state.suppress(&config, package),
    );
    let package = store.package(&renewed, removed_at + 1).unwrap();
    assert!(state.authorization(&config, package).is_none());
    let package = store
        .preference_package(&renewed, removed_at + 1, true)
        .unwrap();
    drop(store);
    assert!(!application_sharing_enabled(&config, "com.example.Shield_App").unwrap());
    assert!(!dir.path().join("config.toml").exists());
    state.preference(&config, &package, true).unwrap();
    state.sent(&in_flight, true, removed_at);
    state.complete(
        &config,
        "重开后完成",
        TaskKind::Sign,
        TaskStatus::Succeeded,
        false,
        removed_at,
    );
    assert!(state.next(&config, removed_at + 30).is_none());
    state.enqueue(
        &config,
        identity(),
        state.generation("com.example.Shield_App"),
        "sign",
        "2026-09-18".into(),
        removed_at + 30,
    );
    assert_eq!(state.pending_count(), 1);
}

fn identity() -> ApplicationIdentity {
    ApplicationIdentity {
        package_name: "com.example.Shield_App".into(),
        app_name: Some("示例应用".into()),
        app_version_code: Some("9223372036854775807".into()),
    }
}

fn receive_http_body(listener: &std::net::TcpListener, response: &str, delay_ms: u64) -> Vec<u8> {
    use std::io::{Read, Write};
    use std::time::Duration;

    let (mut stream, _) = listener.accept().unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    let mut request = Vec::new();
    let body = loop {
        let mut block = [0; 1024];
        let count = stream.read(&mut block).unwrap();
        assert_ne!(count, 0);
        request.extend_from_slice(&block[..count]);
        let Some(end) = request.windows(4).position(|part| part == b"\r\n\r\n") else {
            continue;
        };
        let headers = String::from_utf8_lossy(&request[..end]).to_ascii_lowercase();
        let length = headers
            .lines()
            .find_map(|line| {
                line.strip_prefix("content-length:")
                    .map(|value| value.trim().parse::<usize>().unwrap())
            })
            .unwrap();
        if request.len() >= end + 4 + length {
            break request[end + 4..end + 4 + length].to_vec();
        }
    };
    std::thread::sleep(Duration::from_millis(delay_ms));
    let _ = write!(
        stream,
        "HTTP/1.1 {response}\r\nContent-Length: 0\r\nLocation: http://127.0.0.1:1/forbidden\r\nConnection: close\r\n\r\n"
    );
    body
}

#[test]
fn application_十字段协议与共享样本一致() {
    let payload = protocol::Submission::new(
        identity(),
        "protect_with_sign",
        "2026-09-18".into(),
        "1.4.0-beta.6".into(),
    );
    let mut value = serde_json::to_value(payload).unwrap();
    value["submission_id"] = serde_json::json!("123e4567-e89b-42d3-a456-426614174000");
    let expected: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../../tests/fixtures/diagnostics/application-usage-v1.json"
    ))
    .unwrap();
    assert_eq!(value, expected);
}

#[test]
fn application_撤销再开启不恢复旧任务或旧重试() {
    let dir = tempfile::tempdir().unwrap();
    let config = AppConfigState::new(dir.path().join("config.toml"), AppConfig::default());
    let state = SharingState::default();
    let old = state.generation("com.example.Shield_App");
    state.enqueue(
        &config,
        identity(),
        old,
        "protect",
        "2026-09-18".into(),
        100,
    );
    assert_eq!(state.pending_count(), 1);
    let in_flight = state.next(&config, 100).unwrap();
    state
        .preference(&config, "com.example.Shield_App", false)
        .unwrap();
    state
        .preference(&config, "com.example.Shield_App", true)
        .unwrap();
    state.sent(&in_flight, true, 101);
    state.enqueue(
        &config,
        identity(),
        old,
        "protect",
        "2026-09-18".into(),
        101,
    );
    assert_eq!(state.pending_count(), 0);
    state.enqueue(
        &config,
        identity(),
        state.generation("com.example.Shield_App"),
        "sign",
        "2026-09-18".into(),
        101,
    );
    assert_eq!(state.pending_count(), 1);
}

#[test]
fn application_检查引用有界释放失效且超大正文不入队() {
    use super::inspection::{FileIdentity, InspectionStore};
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("input.apk");
    std::fs::write(&path, "测试输入").unwrap();
    let mut store = InspectionStore::default();
    let mut ids = Vec::new();
    for now in 100..170 {
        ids.push(store.insert(
            path.clone(),
            FileIdentity::read(&path).unwrap(),
            identity(),
            now,
            |_| {},
        ));
    }
    assert!(store.package(&ids[0], 170).is_none());
    assert!(store.package(&ids[69], 170).is_some());
    store.release(&ids[69]);
    assert!(store.package(&ids[69], 170).is_none());
    assert!(InspectionStore::read_input(&path).is_none());
    let state = SharingState::default();
    let config = AppConfigState::new(dir.path().join("config.toml"), AppConfig::default());
    let mut oversized = identity();
    oversized.app_name = Some("字".repeat(3000));
    state.enqueue(&config, oversized, 0, "protect", "2026-09-18".into(), 100);
    assert_eq!(state.pending_count(), 0);
    let forged = serde_json::from_value::<SharingChoice>(
        serde_json::json!({ "inspectionId": "任意", "enabled": true, "packageName": "com.forged" }),
    );
    assert!(forged.is_err());
}

#[test]
fn application_输入在任务开始后被替换即使恢复也不再分享() {
    use super::inspection::{FileIdentity, InspectionStore};
    use crate::task_manager::{TaskKind, TaskStatus};
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("input.apk");
    std::fs::write(&path, "旧输入").unwrap();
    let config = AppConfigState::new(dir.path().join("config.toml"), AppConfig::default());
    let state = SharingState::default();
    let mut store = InspectionStore::default();
    let id = store.insert(
        path.clone(),
        FileIdentity::read(&path).unwrap(),
        identity(),
        100,
        |_| {},
    );
    let choice = SharingChoice {
        inspection_id: id,
        enabled: true,
    };
    let mut snapshot = store
        .freeze(&state, &config, &path, Some(&choice), "sign", 101)
        .unwrap();
    std::fs::write(&path, "更换后的输入").unwrap();
    snapshot.verify_input();
    std::fs::write(&path, "旧输入").unwrap();
    snapshot.verify_input();
    assert!(snapshot
        .submission(TaskKind::Sign, TaskStatus::Succeeded, false, 102)
        .is_none());
}

#[test]
fn application_队列重试通过本地http保持同一正文且不再第三次发送() {
    use std::{net::TcpListener, time::Duration};
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        let mut bodies = Vec::new();
        for _ in 0..2 {
            bodies.push(receive_http_body(&listener, "503 Unavailable", 0));
        }
        bodies
    });
    let dir = tempfile::tempdir().unwrap();
    let config = AppConfigState::new(dir.path().join("config.toml"), AppConfig::default());
    let state = SharingState::default();
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let client = transport::client(Duration::from_secs(1)).unwrap();
    state.enqueue(&config, identity(), 0, "protect", "2026-09-18".into(), 100);
    for now in [100, 130] {
        let item = state.next(&config, now).unwrap();
        let retry = runtime.block_on(transport::send(
            &client,
            &format!("http://{address}/reports/application-usage"),
            &item.payload,
        ));
        state.sent(&item, retry, now);
    }
    assert!(state.next(&config, 160).is_none());
    let bodies = server.join().unwrap();
    assert_eq!(bodies[0], bodies[1]);
}

#[test]
fn application_队列有界且过期不发送() {
    let dir = tempfile::tempdir().unwrap();
    let config = AppConfigState::new(dir.path().join("config.toml"), AppConfig::default());
    let state = SharingState::default();
    for _ in 0..40 {
        state.enqueue(&config, identity(), 0, "protect", "2026-09-18".into(), 100);
    }
    assert_eq!(state.pending_count(), 32);
    assert!(state.next(&config, 400).is_none());
    assert_eq!(state.pending_count(), 0);
}

#[test]
fn application_失败保存禁止发送且重试最多一次并保留编号() {
    let dir = tempfile::tempdir().unwrap();
    let config = AppConfigState::new(dir.path().join("config.toml"), AppConfig::default());
    let state = SharingState::default();
    state.enqueue(&config, identity(), 0, "sign", "2026-09-18".into(), 100);
    let first = state.next(&config, 100).unwrap();
    state.sent(&first, true, 100);
    assert!(state.next(&config, 129).is_none());
    let retry = state.next(&config, 130).unwrap();
    assert_eq!(first.payload.submission_id, retry.payload.submission_id);
    state.sent(&retry, true, 130);
    assert!(state.next(&config, 160).is_none());
    let broken = AppConfigState::new(dir.path().to_path_buf(), AppConfig::default());
    assert!(state
        .preference(&broken, "com.example.Shield_App", true)
        .is_err());
    state.enqueue(
        &broken,
        identity(),
        state.generation("com.example.Shield_App"),
        "sign",
        "2026-09-18".into(),
        200,
    );
    assert_eq!(state.pending_count(), 0);
}

#[test]
fn application_输入替换与检查过期失效且原地写回不回查() {
    use super::inspection::{FileIdentity, InspectionStore};
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("input.apk");
    std::fs::write(&path, "测试输入").unwrap();
    let config = AppConfigState::new(dir.path().join("config.toml"), AppConfig::default());
    let state = SharingState::default();
    let mut store = InspectionStore::default();
    let reference = store.insert(
        path.clone(),
        FileIdentity::read(&path).unwrap(),
        identity(),
        100,
        |_| {},
    );
    let choice = SharingChoice {
        inspection_id: reference.clone(),
        enabled: true,
    };
    assert!(store
        .freeze(&state, &config, &path, Some(&choice), "sign", 400)
        .is_none());
    let mut snapshot = store
        .freeze(&state, &config, &path, Some(&choice), "sign", 101)
        .unwrap();
    snapshot.verify_input();
    std::fs::write(&path, "合法签名写回").unwrap();
    // 复核完成后的终态消费只使用冻结元数据，不读取被签名写回的路径。
    assert!(snapshot
        .submission(
            crate::task_manager::TaskKind::Sign,
            crate::task_manager::TaskStatus::Succeeded,
            false,
            200
        )
        .is_some());
    assert!(store
        .freeze(&state, &config, &path, Some(&choice), "sign", 101)
        .is_none());
}

#[test]
fn application_终态操作与成功日由任务事实决定() {
    use super::inspection::Frozen;
    use crate::task_manager::{TaskKind, TaskStatus};
    let day = 1_789_775_999;
    let make = || Frozen::test_snapshot(identity(), "protect_with_sign");
    let mut task = make();
    task.protected(day);
    let value = serde_json::to_value(
        task.submission(TaskKind::Protect, TaskStatus::Failed, true, day + 2)
            .unwrap(),
    )
    .unwrap();
    assert_eq!(value["operation"], "protect");
    assert_eq!(value["flow"], "protect_with_sign");
    assert_eq!(value["success_date"], "2026-09-18");
    assert!(make()
        .submission(TaskKind::Protect, TaskStatus::Failed, false, day)
        .is_none());
    assert!(make()
        .submission(TaskKind::Sign, TaskStatus::Failed, false, day)
        .is_none());
    let mut cancelled = make();
    cancelled.protected(day);
    assert!(cancelled
        .submission(TaskKind::Protect, TaskStatus::Cancelled, true, day)
        .is_none());
}

#[test]
fn application_过期引用取消可安全续验但替换输入不可续验() {
    use super::inspection::{FileIdentity, InspectionStore};
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("input.apk");
    std::fs::write(&path, "旧输入").unwrap();
    let mut store = InspectionStore::default();
    let id = store.insert(
        path.clone(),
        FileIdentity::read(&path).unwrap(),
        identity(),
        100,
        |_| {},
    );
    assert!(store.package(&id, 401).is_none());
    assert_eq!(
        store.preference_package(&id, 401, true),
        Some("com.example.Shield_App".into())
    );
    std::fs::write(&path, "替换后的其他输入").unwrap();
    assert!(store.preference_package(&id, 402, true).is_none());
    assert_eq!(
        store.preference_package(&id, 402, false),
        Some("com.example.Shield_App".into())
    );
}

#[test]
fn application_重复终态只入队一次且任务不上传() {
    use super::inspection::Frozen;
    use crate::task_manager::{TaskKind, TaskStatus};
    let dir = tempfile::tempdir().unwrap();
    let config = AppConfigState::new(dir.path().join("config.toml"), AppConfig::default());
    let state = SharingState::default();
    let mut snapshot = Frozen::test_snapshot(identity(), "protect");
    snapshot.protected(1_789_775_999);
    state
        .inspections
        .lock()
        .unwrap()
        .tasks
        .insert("本地任务编号".into(), snapshot);
    state.complete(
        &config,
        "本地任务编号",
        TaskKind::Protect,
        TaskStatus::Succeeded,
        true,
        100,
    );
    state.complete(
        &config,
        "本地任务编号",
        TaskKind::Protect,
        TaskStatus::Succeeded,
        true,
        100,
    );
    assert_eq!(state.pending_count(), 1);
    let serialized = serde_json::to_string(&state.next(&config, 100).unwrap().payload).unwrap();
    assert!(!serialized.contains("本地任务编号"));
}

#[test]
fn application_并发撤销与发送决策串行且重新开启不能复活旧代次() {
    use std::sync::{Arc, Barrier};
    let dir = tempfile::tempdir().unwrap();
    let config = Arc::new(AppConfigState::new(
        dir.path().join("config.toml"),
        AppConfig::default(),
    ));
    let state = SharingState::default();
    let barrier = Arc::new(Barrier::new(2));
    let sender = {
        let state = state.clone();
        let config = config.clone();
        let barrier = barrier.clone();
        std::thread::spawn(move || {
            for _ in 0..20 {
                barrier.wait();
                state.enqueue(&config, identity(), 0, "sign", "2026-09-18".into(), 100);
                barrier.wait();
            }
        })
    };
    for _ in 0..20 {
        barrier.wait();
        state
            .preference(&config, "com.example.Shield_App", false)
            .unwrap();
        state
            .preference(&config, "com.example.Shield_App", true)
            .unwrap();
        barrier.wait();
        assert_eq!(state.pending_count(), 0);
    }
    sender.join().unwrap();
}

#[test]
fn application_http状态重定向超时与精确正文() {
    use std::{net::TcpListener, time::Duration};
    let runtime = tokio::runtime::Runtime::new().unwrap();
    for (response, delay, retry) in [
        ("201 Created", 0, false),
        ("503 Unavailable", 0, true),
        ("429 Too Many Requests", 0, false),
        ("302 Found", 0, false),
        ("201 Created", 200, true),
    ] {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || receive_http_body(&listener, response, delay));
        let payload = protocol::Submission::new(
            identity(),
            "sign",
            "2026-09-18".into(),
            "1.4.0-beta.5".into(),
        );
        let client = transport::client(Duration::from_millis(100)).unwrap();
        assert_eq!(
            runtime.block_on(transport::send(
                &client,
                &format!("http://{address}"),
                &payload
            )),
            retry
        );
        let body: serde_json::Value = serde_json::from_slice(&server.join().unwrap()).unwrap();
        assert_eq!(body.as_object().unwrap().len(), 10);
        assert_eq!(body["package_name"], "com.example.Shield_App");
        assert_eq!(body["operation"], "sign");
    }
}

#[test]
#[ignore = "需要 SHIELD_TEST_STANDARD_APK 与 SHIELD_TEST_API19_APK 指向仓库自建 APK，手动执行"]
fn application_真实样本身份与本地_http_十字段环境回归() {
    use std::{net::TcpListener, path::PathBuf, time::Duration};

    let standard = PathBuf::from(
        std::env::var("SHIELD_TEST_STANDARD_APK").expect("需要 SHIELD_TEST_STANDARD_APK"),
    );
    let api19 =
        PathBuf::from(std::env::var("SHIELD_TEST_API19_APK").expect("需要 SHIELD_TEST_API19_APK"));
    let samples = [
        (
            standard,
            "dev.mocika.shield.smoke",
            Some("Mocika Shield Smoke"),
            "protect_with_sign",
        ),
        (
            api19,
            "dev.mocika.shield.api19probe",
            Some("Mocika API 19 Native 探针"),
            "sign",
        ),
    ];

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let sample_count = samples.len();
    let server = std::thread::spawn(move || {
        let mut bodies = Vec::new();
        for _ in 0..sample_count {
            bodies.push(receive_http_body(&listener, "201 Created", 0));
        }
        bodies
    });

    let runtime = tokio::runtime::Runtime::new().unwrap();
    let client = transport::client(Duration::from_secs(1)).unwrap();
    let mut expected = Vec::new();
    for (path, package, name, flow) in samples {
        let (_, identity) = inspection::InspectionStore::read_input(&path)
            .unwrap_or_else(|| panic!("生产身份入口应识别自建样本：{}", path.display()));
        assert_eq!(identity.package_name, package);
        assert_eq!(identity.app_name.as_deref(), name);
        assert_eq!(identity.app_version_code.as_deref(), Some("1"));
        let payload = protocol::Submission::new(
            identity,
            flow,
            "2026-09-18".into(),
            env!("CARGO_PKG_VERSION").into(),
        );
        assert!(!runtime.block_on(transport::send(
            &client,
            &format!("http://{address}/reports/application-usage"),
            &payload,
        )));
        expected.push((path, package, name, flow));
    }

    let bodies = server.join().unwrap();
    assert_eq!(bodies.len(), expected.len());
    let allowed = [
        "app_name",
        "app_version_code",
        "flow",
        "notice_version",
        "operation",
        "package_name",
        "schema_version",
        "submission_id",
        "success_date",
        "tool_version",
    ];
    for (body, (path, package, name, flow)) in bodies.iter().zip(expected) {
        let value: serde_json::Value = serde_json::from_slice(body).unwrap();
        let object = value.as_object().unwrap();
        let mut keys = object.keys().map(String::as_str).collect::<Vec<_>>();
        keys.sort_unstable();
        assert_eq!(keys, allowed);
        assert_eq!(value["package_name"], package);
        assert_eq!(value["app_name"].as_str(), name);
        assert_eq!(value["app_version_code"], "1");
        assert_eq!(value["tool_version"], env!("CARGO_PKG_VERSION"));
        assert_eq!(value["flow"], flow);
        assert_eq!(
            value["operation"],
            if flow == "sign" { "sign" } else { "protect" }
        );
        let serialized = String::from_utf8_lossy(body);
        assert!(!serialized.contains(path.to_string_lossy().as_ref()));
        for forbidden in ["task_id", "certificate", "keystore", "password", "error"] {
            assert!(!object.contains_key(forbidden));
        }
    }
}

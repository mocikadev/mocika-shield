use super::*;
use crate::app_config::AppConfig;
use shield_core::diagnostic::{Diagnostic, FailureCode};

fn identity(name: &str, revision: &str) -> BuildIdentity {
    BuildIdentity::from_build(Some(name), "dev.example.tool", Some(revision)).unwrap()
}

#[test]
fn 同版改名及换提交分桶且不重归属旧积压() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    let today = today_utc();
    let mut config = AppConfig::default();
    config.telemetry.daily.insert(
        daily_key(&today, app_version()),
        DailyTelemetry {
            usage_date: today.clone(),
            app_version: app_version().into(),
            protect_success_count: 4,
            ..Default::default()
        },
    );
    let first = AppConfigState::new(path.clone(), config)
        .with_telemetry_identity(Some(identity("工具甲", "abcdef01")));
    record_app_start(&first);
    record_event(&first, TelemetryEvent::ProtectSucceeded);
    let second = AppConfigState::new(path.clone(), AppConfig::default())
        .with_telemetry_identity(Some(identity("工具乙", "abcdef01")));
    record_event(&second, TelemetryEvent::ProtectSucceeded);
    let rebuilt = AppConfigState::new(path, AppConfig::default())
        .with_telemetry_identity(Some(identity("工具甲", "abcdef02")));
    record_failure(&rebuilt, FailureStage::SignExecute);
    record_reason(
        &rebuilt,
        FailureContext::sign("SignApk"),
        Diagnostic::new(FailureCode::SigningFailed),
    );
    let saved = first.read().unwrap().telemetry;
    assert_eq!(saved.daily.len(), 1);
    assert_eq!(
        saved.daily.values().next().unwrap().protect_success_count,
        4
    );
    assert_eq!(saved.daily_sources.len(), 3);
    let pending = pending_snapshots(&saved, &today);
    assert_eq!(pending.len(), 4);
    let legacy = pending.iter().find(|(aware, _, _)| !aware).unwrap();
    let old_payload =
        serde_json::to_value(daily_payload(saved.anonymous_id.clone(), legacy.2.clone())).unwrap();
    assert!(old_payload.get("build_identity").is_none());
    assert!(old_payload.get("schema_version").is_none());
    let first_snapshot = pending
        .iter()
        .find(|(_, _, item)| item.build_identity.as_ref() == Some(&identity("工具甲", "abcdef01")))
        .unwrap();
    let payload = serde_json::to_value(daily_payload(
        saved.anonymous_id.clone(),
        first_snapshot.2.clone(),
    ))
    .unwrap();
    assert_eq!(payload["schema_version"], 3);
    assert_eq!(payload["build_identity"]["product_name"], "工具甲");
    assert_eq!(payload["protect_success_count"], 1);
    assert_eq!(
        saved
            .daily_sources
            .values()
            .map(|item| item.sign_failed_count)
            .sum::<u32>(),
        1
    );
    assert_eq!(
        saved
            .daily_sources
            .values()
            .map(|item| item.failure_reason_counts.len())
            .sum::<usize>(),
        1
    );
}

#[test]
fn 旧模型差异保存不会合并或删除来源分区() {
    #[derive(Clone, serde::Serialize, serde::Deserialize)]
    struct OldConfig {
        telemetry: OldTelemetry,
    }
    #[derive(Clone, serde::Serialize, serde::Deserialize)]
    struct OldTelemetry {
        enabled: bool,
    }
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    let state = AppConfigState::new(path.clone(), AppConfig::default())
        .with_telemetry_identity(Some(identity("工具", "abcdef01")));
    record_event(&state, TelemetryEvent::ProtectSucceeded);
    let before = toml::to_string(&state.read().unwrap().telemetry.daily_sources).unwrap();
    let old = shield_services::config::ConfigStore::existing(path);
    old.update(
        &OldConfig {
            telemetry: OldTelemetry { enabled: true },
        },
        |value| {
            value.telemetry.enabled = false;
            Ok(())
        },
    )
    .unwrap();
    assert_eq!(
        toml::to_string(&state.read().unwrap().telemetry.daily_sources).unwrap(),
        before
    );
    record_app_start(&state);
    record_event(&state, TelemetryEvent::ProtectSucceeded);
    record_failure(&state, FailureStage::SignExecute);
    record_reason(
        &state,
        FailureContext::sign("SignApk"),
        Diagnostic::new(FailureCode::SigningFailed),
    );
    tauri::async_runtime::block_on(sync_pending(&state));
    assert_eq!(
        toml::to_string(&state.read().unwrap().telemetry.daily_sources).unwrap(),
        before
    );
}

#[test]
fn 上传确认只清理对应来源分区历史且保留当天累计() {
    let dir = tempfile::tempdir().unwrap();
    let mut config = AppConfig::default();
    let past = "2026-01-01";
    let today = "2026-01-02";
    let source = identity("工具", "abcdef01");
    daily_entry(&mut config.telemetry.daily, past, "1.0.0", None);
    daily_entry(
        &mut config.telemetry.daily_sources,
        past,
        "1.0.0",
        Some(&source),
    );
    daily_entry(
        &mut config.telemetry.daily_sources,
        today,
        "1.0.0",
        Some(&source),
    );
    let key = format!("{}|{}", daily_key(past, "1.0.0"), source.key());
    let state = AppConfigState::new(dir.path().join("config.toml"), config);
    mark_uploaded(&state, &key, today, true);
    let saved = state.read().unwrap().telemetry;
    assert_eq!(saved.daily.len(), 1);
    assert_eq!(saved.daily_sources.len(), 1);
    let key = saved.daily_sources.keys().next().unwrap();
    mark_uploaded(&state, key, today, true);
    assert_eq!(state.read().unwrap().telemetry.daily_sources.len(), 1);
}

#[test]
fn 损坏的来源积压不回退旧入口或套用当前名称() {
    let mut config = AppConfig::default();
    config.telemetry.daily_sources.insert(
        "bad".into(),
        DailyTelemetry {
            usage_date: "2026-01-01".into(),
            app_version: "1.0.0".into(),
            build_identity: None,
            ..Default::default()
        },
    );
    assert!(pending_snapshots(&config.telemetry, "2026-01-01").is_empty());
    assert_eq!(config.telemetry.daily_sources.len(), 1);
    assert_ne!(SOURCE_TELEMETRY_URL, TELEMETRY_URL);
    assert!(SOURCE_TELEMETRY_URL.ends_with("/events/daily-v3"));
}

#[test]
fn 新快照限额不归零或改变本地累计() {
    let item = DailyTelemetry {
        app_start_count: 10_001,
        build_identity: Some(identity("工具", "abcdef01")),
        ..Default::default()
    };
    let payload = daily_payload("test".into(), item.clone());
    assert_eq!(payload.app_start_count, 10_000);
    assert_eq!(item.app_start_count, 10_001);
}

#[test]
fn 本地_http证明快照不改名且失败不回退旧入口() {
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::time::{Duration, Instant};
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let address = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(8);
        let mut requests = Vec::new();
        while requests.len() < 2 {
            let (mut stream, _) = match listener.accept() {
                Ok(stream) => stream,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    assert!(Instant::now() < deadline, "本地接收超时");
                    std::thread::sleep(Duration::from_millis(5));
                    continue;
                }
                Err(error) => panic!("本地接收失败：{error}"),
            };
            stream
                .set_read_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            let mut bytes = Vec::new();
            let (header_end, length) = loop {
                let mut block = [0; 2048];
                let count = stream.read(&mut block).unwrap();
                assert!(count > 0);
                bytes.extend_from_slice(&block[..count]);
                if let Some(end) = bytes.windows(4).position(|part| part == b"\r\n\r\n") {
                    let header = String::from_utf8(bytes[..end].to_vec())
                        .unwrap()
                        .to_lowercase();
                    let length = header
                        .lines()
                        .find_map(|line| line.strip_prefix("content-length:"))
                        .unwrap()
                        .trim()
                        .parse::<usize>()
                        .unwrap();
                    break (end + 4, length);
                }
            };
            while bytes.len() < header_end + length {
                let mut block = [0; 2048];
                let count = stream.read(&mut block).unwrap();
                assert!(count > 0);
                bytes.extend_from_slice(&block[..count]);
            }
            let header = String::from_utf8(bytes[..header_end].to_vec()).unwrap();
            let source = header.starts_with("POST /events/daily-v3 ");
            let status = if source {
                "503 Service Unavailable"
            } else {
                "204 No Content"
            };
            write!(
                stream,
                "HTTP/1.1 {status}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
            )
            .unwrap();
            let body: serde_json::Value =
                serde_json::from_slice(&bytes[header_end..header_end + length]).unwrap();
            requests.push((source, body));
        }
        requests
    });
    let dir = tempfile::tempdir().unwrap();
    let mut config = AppConfig::default();
    let yesterday = today_utc_days_ago(1);
    daily_entry(&mut config.telemetry.daily, &yesterday, "1.0.0", None);
    daily_entry(
        &mut config.telemetry.daily_sources,
        &yesterday,
        "1.0.0",
        Some(&identity("历史名称", "abcdef01")),
    );
    let state = AppConfigState::new(dir.path().join("config.toml"), config)
        .with_telemetry_identity(Some(identity("当前名称", "abcdef02")));
    tauri::async_runtime::block_on(sync_pending_to(
        &state,
        &format!("http://{address}/events/daily"),
        &format!("http://{address}/events/daily-v3"),
    ));
    let requests = server.join().unwrap();
    assert_eq!(requests.len(), 2);
    assert!(requests[0].1.get("build_identity").is_none());
    assert_eq!(requests[1].1["build_identity"]["product_name"], "历史名称");
    let saved = state.read().unwrap().telemetry;
    assert!(saved.daily.is_empty());
    assert_eq!(
        saved.daily_sources.len(),
        1,
        "503 后须保留来源积压，不能回退旧入口"
    );
}

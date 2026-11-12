use crate::app_config::{AppConfigState, DailyTelemetry, TelemetryConfig};
use crate::build_identity::BuildIdentity;
use crate::failure_diagnostic::{FailureContext, FailureReasonCount};
use chrono_like::today_utc;
use serde::Serialize;
use shield_core::diagnostic::{Diagnostic, CLASSIFIER_VERSION};
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tauri::Manager;

const EVENT_SYNC_DELAY: Duration = Duration::from_secs(30);
const TELEMETRY_URL: &str = "https://mocika-shield-stats-api.xuechao-suo.workers.dev/events/daily";
const SOURCE_TELEMETRY_URL: &str =
    "https://mocika-shield-stats-api.xuechao-suo.workers.dev/events/daily-v3";

#[derive(Default)]
pub(crate) struct TelemetryRuntime {
    sync_scheduled: AtomicBool,
}

/// 桌面端内部事件；不允许调用方以字符串拼写持久化字段。
#[derive(Clone, Copy)]
pub(crate) enum TelemetryEvent {
    ProtectStarted,
    ProtectSucceeded,
    SignSucceeded,
}

/// 仅记录固定分类，不持久化或上传原始错误、路径和 APK 信息。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FailureStage {
    ProtectPrepare,
    ProtectUnpack,
    ProtectManifest,
    ProtectDexRuntime,
    ProtectAlign,
    ProtectSign,
    SignPrepare,
    SignAlign,
    SignExecute,
    TaskCancelled,
    TaskUnknown,
}

impl FailureStage {
    fn code(self) -> &'static str {
        match self {
            Self::ProtectPrepare => "protect_prepare",
            Self::ProtectUnpack => "protect_unpack",
            Self::ProtectManifest => "protect_manifest",
            Self::ProtectDexRuntime => "protect_dex_runtime",
            Self::ProtectAlign => "protect_align",
            Self::ProtectSign => "protect_sign",
            Self::SignPrepare => "sign_prepare",
            Self::SignAlign => "sign_align",
            Self::SignExecute => "sign_execute",
            Self::TaskCancelled => "task_cancelled",
            Self::TaskUnknown => "task_unknown",
        }
    }

    fn increments_protect_failure(self) -> bool {
        matches!(
            self,
            Self::ProtectPrepare
                | Self::ProtectUnpack
                | Self::ProtectManifest
                | Self::ProtectDexRuntime
                | Self::ProtectAlign
        )
    }

    fn increments_sign_failure(self) -> bool {
        matches!(
            self,
            Self::ProtectSign | Self::SignPrepare | Self::SignAlign | Self::SignExecute
        )
    }

    fn operation_and_stage(self) -> (&'static str, &'static str) {
        match self {
            Self::ProtectPrepare => ("protect", "prepare"),
            Self::ProtectUnpack => ("protect", "unpack"),
            Self::ProtectManifest => ("protect", "manifest"),
            Self::ProtectDexRuntime => ("protect", "dex_runtime"),
            Self::ProtectAlign => ("protect", "align"),
            Self::ProtectSign => ("protect", "sign"),
            Self::SignPrepare => ("sign", "prepare"),
            Self::SignAlign => ("sign", "align"),
            Self::SignExecute => ("sign", "execute"),
            Self::TaskCancelled => ("task", "cancelled"),
            Self::TaskUnknown => ("task", "unknown"),
        }
    }
}

pub(crate) fn protect_failure_stage(step: &str, signing_started: bool) -> FailureStage {
    if signing_started {
        return match step {
            "PrepareSign" => FailureStage::ProtectSign,
            "AlignApk" => FailureStage::ProtectSign,
            "SignApk" | "Cleanup" => FailureStage::ProtectSign,
            _ => FailureStage::TaskUnknown,
        };
    }
    match step {
        "CheckTools" => FailureStage::ProtectPrepare,
        "Unpack" => FailureStage::ProtectUnpack,
        "ModifyManifest" | "Repack" => FailureStage::ProtectManifest,
        "ProcessDex" | "InjectRuntime" => FailureStage::ProtectDexRuntime,
        "AlignApk" => FailureStage::ProtectAlign,
        _ => FailureStage::TaskUnknown,
    }
}

pub(crate) fn sign_failure_stage(step: &str) -> FailureStage {
    match step {
        "PrepareSign" => FailureStage::SignPrepare,
        "AlignApk" => FailureStage::SignAlign,
        "SignApk" => FailureStage::SignExecute,
        _ => FailureStage::TaskUnknown,
    }
}

pub(crate) fn record_app_start(state: &AppConfigState) {
    let today = today_utc();
    let _ = state.mutate(|config| {
        if !config.telemetry.enabled {
            return;
        }
        normalize_daily_entries(&mut config.telemetry.daily);
        let entry = current_entry(state, &mut config.telemetry, &today);
        entry.app_start_count = entry.app_start_count.saturating_add(1);
        let cutoff = today_utc_days_ago(30);
        config
            .telemetry
            .daily
            .retain(|_, item| item.usage_date >= cutoff && !item.uploaded);
        config
            .telemetry
            .daily_sources
            .retain(|_, item| item.usage_date >= cutoff && !item.uploaded);
    });
}

pub(crate) fn record_event(state: &AppConfigState, event: TelemetryEvent) {
    let today = today_utc();
    let _ = state.mutate(|config| {
        if !config.telemetry.enabled {
            return;
        }
        normalize_daily_entries(&mut config.telemetry.daily);
        let entry = current_entry(state, &mut config.telemetry, &today);
        match event {
            TelemetryEvent::ProtectStarted => {
                entry.protect_start_count = entry.protect_start_count.saturating_add(1)
            }
            TelemetryEvent::ProtectSucceeded => {
                entry.protect_success_count = entry.protect_success_count.saturating_add(1)
            }
            TelemetryEvent::SignSucceeded => {
                entry.sign_success_count = entry.sign_success_count.saturating_add(1)
            }
        }
    });
}

pub(crate) fn record_failure(state: &AppConfigState, stage: FailureStage) {
    let today = today_utc();
    let _ = state.mutate(|config| {
        if !config.telemetry.enabled {
            return;
        }
        normalize_daily_entries(&mut config.telemetry.daily);
        let entry = current_entry(state, &mut config.telemetry, &today);
        let count = entry
            .failure_counts
            .entry(stage.code().to_string())
            .or_default();
        *count = count.saturating_add(1);
        if stage.increments_protect_failure() {
            entry.protect_failed_count = entry.protect_failed_count.saturating_add(1);
        }
        if stage.increments_sign_failure() {
            entry.sign_failed_count = entry.sign_failed_count.saturating_add(1);
        }
    });
}

pub(crate) fn record_reason(
    state: &AppConfigState,
    context: FailureContext,
    diagnostic: Diagnostic,
) {
    let _ = state.mutate(|config| {
        if !config.telemetry.enabled {
            return;
        }
        let entry = current_entry(state, &mut config.telemetry, &today_utc());
        merge_reason(
            &mut entry.failure_reason_counts,
            FailureReasonCount {
                context,
                code: diagnostic.code,
                classifier_version: CLASSIFIER_VERSION,
                count: 1,
            },
        );
    });
}

fn merge_reason(counts: &mut Vec<FailureReasonCount>, source: FailureReasonCount) {
    if let Some(existing) = counts.iter_mut().find(|item| {
        item.context == source.context
            && item.code == source.code
            && item.classifier_version == source.classifier_version
    }) {
        existing.count = existing.count.saturating_add(source.count).min(10_000);
    } else if counts.len() < 128 {
        counts.push(source);
    }
}

fn app_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

pub(crate) fn daily_key(usage_date: &str, app_version: &str) -> String {
    format!("{usage_date}|{app_version}")
}

fn daily_entry<'a>(
    daily: &'a mut BTreeMap<String, DailyTelemetry>,
    usage_date: &str,
    version: &str,
    identity: Option<&BuildIdentity>,
) -> &'a mut DailyTelemetry {
    let key = match identity {
        Some(identity) => format!("{}|{}", daily_key(usage_date, version), identity.key()),
        None => daily_key(usage_date, version),
    };
    let entry = daily.entry(key).or_default();
    entry.usage_date = usage_date.to_string();
    entry.app_version = version.to_string();
    entry.build_identity = identity.cloned();
    entry.failure_classifier_version = Some(CLASSIFIER_VERSION);
    entry
}

fn current_entry<'a>(
    state: &AppConfigState,
    telemetry: &'a mut TelemetryConfig,
    date: &str,
) -> &'a mut DailyTelemetry {
    let identity = state.telemetry_identity();
    let daily = if identity.is_some() {
        &mut telemetry.daily_sources
    } else {
        &mut telemetry.daily
    };
    daily_entry(daily, date, app_version(), identity)
}

fn normalize_daily_entries(daily: &mut BTreeMap<String, DailyTelemetry>) {
    let entries = std::mem::take(daily);
    for (legacy_key, mut item) in entries {
        let (usage_date, stored_version) = legacy_key
            .split_once('|')
            .map(|(date, version)| (date.to_string(), version.to_string()))
            .unwrap_or_else(|| (legacy_key, app_version().to_string()));
        if item.usage_date.is_empty() {
            item.usage_date = usage_date;
        }
        if item.app_version.is_empty() {
            item.app_version = stored_version;
        }
        let key = daily_key(&item.usage_date, &item.app_version);
        if let Some(existing) = daily.get_mut(&key) {
            merge_daily_telemetry(existing, item);
        } else {
            daily.insert(key, item);
        }
    }
}

fn merge_daily_telemetry(target: &mut DailyTelemetry, source: DailyTelemetry) {
    target.app_start_count = target
        .app_start_count
        .saturating_add(source.app_start_count);
    target.protect_start_count = target
        .protect_start_count
        .saturating_add(source.protect_start_count);
    target.protect_success_count = target
        .protect_success_count
        .saturating_add(source.protect_success_count);
    target.protect_failed_count = target
        .protect_failed_count
        .saturating_add(source.protect_failed_count);
    target.sign_success_count = target
        .sign_success_count
        .saturating_add(source.sign_success_count);
    target.sign_failed_count = target
        .sign_failed_count
        .saturating_add(source.sign_failed_count);
    for (stage, count) in source.failure_counts {
        let total = target.failure_counts.entry(stage).or_default();
        *total = total.saturating_add(count);
    }
    for reason in source.failure_reason_counts {
        merge_reason(&mut target.failure_reason_counts, reason);
    }
    target.failure_classifier_version = target
        .failure_classifier_version
        .or(source.failure_classifier_version);
    target.uploaded &= source.uploaded;
}

#[derive(Serialize)]
struct DailyPayload {
    #[serde(skip_serializing_if = "Option::is_none")]
    schema_version: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    build_identity: Option<BuildIdentity>,
    anonymous_id: String,
    usage_date: String,
    app_version: String,
    platform: String,
    arch: String,
    app_start_count: u32,
    protect_start_count: u32,
    protect_success_count: u32,
    protect_failed_count: u32,
    sign_success_count: u32,
    sign_failed_count: u32,
    failure_counts: Vec<FailureCountPayload>,
    #[serde(skip_serializing_if = "Option::is_none")]
    failure_classifier_version: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    failure_reason_counts: Option<Vec<FailureReasonCount>>,
}

#[derive(Serialize)]
struct FailureCountPayload {
    operation: String,
    stage: String,
    count: u32,
}

pub(crate) async fn sync_pending(state: &AppConfigState) {
    sync_pending_to(state, TELEMETRY_URL, SOURCE_TELEMETRY_URL).await;
}

async fn sync_pending_to(state: &AppConfigState, legacy_url: &str, source_url: &str) {
    let Ok(snapshot) = state.read() else { return };
    if !snapshot.telemetry.enabled {
        return;
    }
    let today = today_utc();
    let pending = pending_snapshots(&snapshot.telemetry, &today);
    if pending.is_empty() {
        return;
    }
    let client = match reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(3))
        .redirect(reqwest::redirect::Policy::none())
        .build()
    {
        Ok(v) => v,
        Err(_) => return,
    };
    for (source_aware, key, item) in pending {
        if !state.read().is_ok_and(|current| current.telemetry.enabled) {
            break;
        }
        let payload = daily_payload(snapshot.telemetry.anonymous_id.clone(), item);
        let url = if source_aware { source_url } else { legacy_url };
        let Ok(response) = client.post(url).json(&payload).send().await else {
            continue;
        };
        if (source_aware && response.status() == reqwest::StatusCode::NO_CONTENT)
            || (!source_aware && response.status().is_success())
        {
            mark_uploaded(state, &key, &today, source_aware);
        }
    }
}

fn pending_snapshots(
    telemetry: &TelemetryConfig,
    today: &str,
) -> Vec<(bool, String, DailyTelemetry)> {
    let mut result = Vec::new();
    for (source_aware, daily) in [(false, &telemetry.daily), (true, &telemetry.daily_sources)] {
        for (key, item) in daily {
            if item.usage_date.as_str() > today || item.uploaded {
                continue;
            }
            let mut snapshot = item.clone();
            if source_aware {
                if !snapshot
                    .build_identity
                    .as_ref()
                    .is_some_and(BuildIdentity::is_valid)
                {
                    continue;
                }
            } else {
                // 旧分区的身份永不从当前构建补齐，也不借额外字段升级协议。
                snapshot.build_identity = None;
            }
            result.push((source_aware, key.clone(), snapshot));
        }
    }
    result
}

fn daily_payload(anonymous_id: String, item: DailyTelemetry) -> DailyPayload {
    // 新协议拒绝超限值；本地累计不归零，只限制单日上报上限。
    let limit = if item.build_identity.is_some() {
        10_000
    } else {
        u32::MAX
    };
    DailyPayload {
        schema_version: item.build_identity.as_ref().map(|_| 3),
        build_identity: item.build_identity.clone(),
        anonymous_id,
        usage_date: item.usage_date.clone(),
        app_version: item.app_version.clone(),
        platform: std::env::consts::OS.to_string(),
        arch: std::env::consts::ARCH.to_string(),
        app_start_count: item.app_start_count.min(limit),
        protect_start_count: item.protect_start_count.min(limit),
        protect_success_count: item.protect_success_count.min(limit),
        protect_failed_count: item.protect_failed_count.min(limit),
        sign_success_count: item.sign_success_count.min(limit),
        sign_failed_count: item.sign_failed_count.min(limit),
        failure_classifier_version: item.failure_classifier_version,
        failure_reason_counts: item
            .failure_classifier_version
            .map(|_| item.failure_reason_counts.clone()),
        failure_counts: item
            .failure_counts
            .iter()
            .map(|(code, count)| FailureCountPayload {
                operation: failure_code_parts(code).0.to_string(),
                stage: failure_code_parts(code).1.to_string(),
                count: (*count).min(limit),
            })
            .collect(),
    }
}

#[cfg(test)]
fn payload_for_test(item: DailyTelemetry) -> DailyPayload {
    daily_payload("00000000-0000-0000-0000-000000000000".into(), item)
}

fn failure_code_parts(code: &str) -> (&str, &str) {
    match code {
        "protect_prepare" => FailureStage::ProtectPrepare.operation_and_stage(),
        "protect_unpack" => FailureStage::ProtectUnpack.operation_and_stage(),
        "protect_manifest" => FailureStage::ProtectManifest.operation_and_stage(),
        "protect_dex_runtime" => FailureStage::ProtectDexRuntime.operation_and_stage(),
        "protect_align" => FailureStage::ProtectAlign.operation_and_stage(),
        "protect_sign" => FailureStage::ProtectSign.operation_and_stage(),
        "sign_prepare" => FailureStage::SignPrepare.operation_and_stage(),
        "sign_align" => FailureStage::SignAlign.operation_and_stage(),
        "sign_execute" => FailureStage::SignExecute.operation_and_stage(),
        "task_cancelled" => FailureStage::TaskCancelled.operation_and_stage(),
        _ => FailureStage::TaskUnknown.operation_and_stage(),
    }
}

fn mark_uploaded(state: &AppConfigState, key: &str, today: &str, source_aware: bool) {
    let _ = state.mutate(|config| {
        let daily = if source_aware {
            &mut config.telemetry.daily_sources
        } else {
            &mut config.telemetry.daily
        };
        if daily
            .get(key)
            .is_some_and(|item| item.usage_date.as_str() < today)
        {
            daily.remove(key);
        }
    });
}

/// 操作事件在本地累计后延迟同步，避免一次操作产生多次网络请求。
pub(crate) fn schedule_sync(app: tauri::AppHandle) {
    let runtime = app.state::<TelemetryRuntime>();
    if runtime.sync_scheduled.swap(true, Ordering::SeqCst) {
        return;
    }
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(EVENT_SYNC_DELAY).await;
        let state = app.state::<AppConfigState>();
        sync_pending(&state).await;
        app.state::<TelemetryRuntime>()
            .sync_scheduled
            .store(false, Ordering::SeqCst);
    });
}

fn today_utc_days_ago(days: i64) -> String {
    chrono_like::days_ago(days)
}

mod chrono_like {
    use std::time::{SystemTime, UNIX_EPOCH};

    pub(super) fn today_utc() -> String {
        date_from_days(days_since_epoch())
    }

    pub(super) fn days_ago(days: i64) -> String {
        date_from_days(days_since_epoch().saturating_sub(days))
    }

    fn days_since_epoch() -> i64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64
            / 86_400
    }

    fn date_from_days(days: i64) -> String {
        let z = days + 719_468;
        let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
        let doe = z - era * 146_097;
        let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
        let y = yoe + era * 400;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let d = doy - (153 * mp + 2) / 5 + 1;
        let m = mp + if mp < 10 { 3 } else { -9 };
        let year = y + if m <= 2 { 1 } else { 0 };
        format!("{year:04}-{m:02}-{d:02}")
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn 旧记录省略原因字段而新记录携带真实分类版本() {
        let old = super::payload_for_test(DailyTelemetry {
            app_version: "1.4.0-beta.4".into(),
            ..DailyTelemetry::default()
        });
        let old_json = serde_json::to_value(old).unwrap();
        assert!(old_json.get("failure_classifier_version").is_none());
        assert!(old_json.get("failure_reason_counts").is_none());

        let mut current = DailyTelemetry {
            failure_classifier_version: Some(1),
            ..Default::default()
        };
        current
            .failure_reason_counts
            .push(crate::failure_diagnostic::FailureReasonCount {
                context: crate::failure_diagnostic::FailureContext::sign("SignApk"),
                code: shield_core::diagnostic::FailureCode::SigningFailed,
                classifier_version: 1,
                count: 1,
            });
        let current_json = serde_json::to_value(super::payload_for_test(current)).unwrap();
        assert_eq!(current_json["failure_classifier_version"], 1);
        assert_eq!(
            current_json["failure_reason_counts"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
    }

    #[test]
    fn 原因分类兼容旧配置且受开关控制() {
        use crate::failure_diagnostic::FailureContext;
        use shield_core::diagnostic::{Diagnostic, FailureCode};
        let old: DailyTelemetry = toml::from_str("app_version = '1.4.0-beta.4'").unwrap();
        assert!(old.failure_reason_counts.is_empty());
        assert!(old.failure_classifier_version.is_none());
        let dir = tempfile::tempdir().unwrap();
        let state = AppConfigState::new(dir.path().join("config.toml"), AppConfig::default());
        let context = FailureContext::sign("SignApk");
        let diagnostic = Diagnostic::new(FailureCode::SigningFailed);
        super::record_reason(&state, context, diagnostic);
        let first = state.read().unwrap();
        let day = first.telemetry.daily.values().next().unwrap();
        assert_eq!(day.failure_reason_counts[0].count, 1);
        assert_eq!(day.failure_classifier_version, Some(1));
        let encoded = toml::to_string(day).unwrap();
        let decoded: DailyTelemetry = toml::from_str(&encoded).unwrap();
        assert_eq!(decoded.failure_reason_counts, day.failure_reason_counts);
        state
            .mutate(|config| config.telemetry.enabled = false)
            .unwrap();
        super::record_reason(&state, context, diagnostic);
        assert_eq!(
            state
                .read()
                .unwrap()
                .telemetry
                .daily
                .values()
                .next()
                .unwrap()
                .failure_reason_counts[0]
                .count,
            1
        );
    }

    use super::{
        daily_key, mark_uploaded, normalize_daily_entries, protect_failure_stage, record_event,
        record_failure, sign_failure_stage, today_utc, FailureStage, TelemetryEvent,
    };
    use crate::app_config::{AppConfig, AppConfigState, DailyTelemetry};

    #[test]
    fn 签名失败会写入每日匿名统计() {
        let dir = tempfile::tempdir().unwrap();
        let state = AppConfigState::new(dir.path().join("config.toml"), AppConfig::default());

        record_failure(&state, FailureStage::SignExecute);

        let total: u32 = state
            .read()
            .unwrap()
            .telemetry
            .daily
            .values()
            .map(|item| item.sign_failed_count)
            .sum();
        assert_eq!(total, 1);
    }

    #[test]
    fn 当天记录会保留完整累计值() {
        let dir = tempfile::tempdir().unwrap();
        let state = AppConfigState::new(dir.path().join("config.toml"), AppConfig::default());

        record_event(&state, TelemetryEvent::ProtectSucceeded);
        record_event(&state, TelemetryEvent::ProtectSucceeded);

        let config = state.read().unwrap();
        let today = today_utc();
        assert_eq!(
            config.telemetry.daily[&daily_key(&today, env!("CARGO_PKG_VERSION"))]
                .protect_success_count,
            2
        );
    }

    #[test]
    fn 同一日期的不同版本分别保存() {
        assert_ne!(
            daily_key("2026-08-25", "1.3.0"),
            daily_key("2026-08-25", "1.4.0-alpha.2")
        );
    }

    #[test]
    fn 失败只记录固定阶段且不保存原始错误() {
        let dir = tempfile::tempdir().unwrap();
        let state = AppConfigState::new(dir.path().join("config.toml"), AppConfig::default());

        record_failure(&state, FailureStage::ProtectManifest);

        let config = state.read().unwrap();
        let entry = config.telemetry.daily.values().next().unwrap();
        assert_eq!(entry.failure_counts.get("protect_manifest"), Some(&1));
        assert_eq!(entry.protect_failed_count, 1);
    }

    #[test]
    fn 取消不计入失败率且核心步骤映射稳定() {
        let dir = tempfile::tempdir().unwrap();
        let state = AppConfigState::new(dir.path().join("config.toml"), AppConfig::default());

        record_failure(&state, FailureStage::TaskCancelled);

        let entry = state
            .read()
            .unwrap()
            .telemetry
            .daily
            .into_values()
            .next()
            .unwrap();
        assert_eq!(entry.failure_counts.get("task_cancelled"), Some(&1));
        assert_eq!(entry.protect_failed_count, 0);
        assert_eq!(entry.sign_failed_count, 0);
        assert_eq!(
            protect_failure_stage("ModifyManifest", false),
            FailureStage::ProtectManifest
        );
        assert_eq!(sign_failure_stage("SignApk"), FailureStage::SignExecute);
        assert_eq!(sign_failure_stage("NotKnown"), FailureStage::TaskUnknown);
    }

    #[test]
    fn 遗留按日期记录会迁移到当前版本键() {
        let mut daily = std::collections::BTreeMap::new();
        daily.insert(
            "2026-08-24".to_string(),
            DailyTelemetry {
                protect_success_count: 2,
                ..DailyTelemetry::default()
            },
        );

        normalize_daily_entries(&mut daily);

        let key = daily_key("2026-08-24", env!("CARGO_PKG_VERSION"));
        assert_eq!(daily[&key].usage_date, "2026-08-24");
        assert_eq!(daily[&key].app_version, env!("CARGO_PKG_VERSION"));
        assert_eq!(daily[&key].protect_success_count, 2);
    }

    #[test]
    fn 上传成功后保留当天并清理此前记录() {
        let dir = tempfile::tempdir().unwrap();
        let mut config = AppConfig::default();
        config
            .telemetry
            .daily
            .insert("2026-07-22".to_string(), DailyTelemetry::default());
        config
            .telemetry
            .daily
            .insert("2026-07-23".to_string(), DailyTelemetry::default());
        let state = AppConfigState::new(dir.path().join("config.toml"), config);

        state
            .mutate(|config| {
                for (key, item) in &mut config.telemetry.daily {
                    item.usage_date = key.clone();
                }
            })
            .unwrap();
        mark_uploaded(&state, "2026-07-22", "2026-07-23", false);
        mark_uploaded(&state, "2026-07-23", "2026-07-23", false);

        let daily = state.read().unwrap().telemetry.daily;
        assert!(!daily.contains_key("2026-07-22"));
        assert!(daily.contains_key("2026-07-23"));
    }
}

#[cfg(test)]
#[path = "telemetry/source_tests.rs"]
mod source_tests;

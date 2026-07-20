use crate::app_config::{AppConfigState, DailyTelemetry};
use chrono_like::today_utc;
use serde::Serialize;

pub(crate) fn record_app_start(state: &AppConfigState) {
    let today = today_utc();
    let _ = state.mutate(|config| {
        let entry = config
            .telemetry
            .daily
            .entry(today.clone())
            .or_insert_with(DailyTelemetry::default);
        entry.app_start_count = entry.app_start_count.saturating_add(1);
        let cutoff = today_utc_days_ago(30);
        config
            .telemetry
            .daily
            .retain(|date, item| date >= &cutoff && !item.uploaded);
    });
}

pub(crate) fn record_event(state: &AppConfigState, field: &str) {
    let today = today_utc();
    let _ = state.mutate(|config| {
        let entry = config.telemetry.daily.entry(today).or_default();
        match field {
            "protect_start_count" => {
                entry.protect_start_count = entry.protect_start_count.saturating_add(1)
            }
            "protect_success_count" => {
                entry.protect_success_count = entry.protect_success_count.saturating_add(1)
            }
            "protect_failed_count" => {
                entry.protect_failed_count = entry.protect_failed_count.saturating_add(1)
            }
            "sign_success_count" => {
                entry.sign_success_count = entry.sign_success_count.saturating_add(1)
            }
            "sign_failed_count" => {
                entry.sign_failed_count = entry.sign_failed_count.saturating_add(1)
            }
            _ => {}
        }
    });
}

#[derive(Serialize)]
struct DailyPayload {
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
}

pub(crate) async fn sync_pending(state: &AppConfigState) {
    let Ok(snapshot) = state.read() else { return };
    if !snapshot.telemetry.enabled {
        return;
    }
    let today = today_utc();
    let pending: Vec<_> = snapshot
        .telemetry
        .daily
        .iter()
        .filter(|(date, item)| date.as_str() < today.as_str() && !item.uploaded)
        .map(|(date, item)| (date.clone(), item.clone()))
        .collect();
    if pending.is_empty() {
        return;
    }
    let client = match reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(3))
        .build()
    {
        Ok(v) => v,
        Err(_) => return,
    };
    for (date, item) in pending {
        let payload = DailyPayload {
            anonymous_id: snapshot.telemetry.anonymous_id.clone(),
            usage_date: date.clone(),
            app_version: env!("CARGO_PKG_VERSION").to_string(),
            platform: std::env::consts::OS.to_string(),
            arch: std::env::consts::ARCH.to_string(),
            app_start_count: item.app_start_count,
            protect_start_count: item.protect_start_count,
            protect_success_count: item.protect_success_count,
            protect_failed_count: item.protect_failed_count,
            sign_success_count: item.sign_success_count,
            sign_failed_count: item.sign_failed_count,
        };
        let Ok(response) = client
            .post("https://mocika-shield-stats-api.xuechao-suo.workers.dev/events/daily")
            .json(&payload)
            .send()
            .await
        else {
            continue;
        };
        if response.status().is_success() {
            let _ = state.mutate(|config| {
                config.telemetry.daily.remove(&date);
            });
        }
    }
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
    use super::record_event;
    use crate::app_config::{AppConfig, AppConfigState};

    #[test]
    fn 签名失败会写入每日匿名统计() {
        let dir = tempfile::tempdir().unwrap();
        let state = AppConfigState::new(dir.path().join("config.toml"), AppConfig::default());

        record_event(&state, "sign_failed_count");

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
}

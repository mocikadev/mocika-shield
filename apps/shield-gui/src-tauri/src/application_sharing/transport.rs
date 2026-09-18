//! 仅发送固定白名单协议；测试接收器不作为生产 IPC 参数。
use super::{
    protocol::{now_seconds, Submission},
    SharingState,
};
use crate::app_config::AppConfigState;
use std::time::Duration;
use tauri::Manager;

const URL: &str =
    "https://mocika-shield-stats-api.xuechao-suo.workers.dev/reports/application-usage";

pub(super) fn client(timeout: Duration) -> Result<reqwest::Client, reqwest::Error> {
    reqwest::Client::builder()
        .timeout(timeout)
        .redirect(reqwest::redirect::Policy::none())
        .build()
}

// 仅网络失败或 5xx 可重试，3xx/4xx（包括 429）直接放弃。
pub(super) async fn send(client: &reqwest::Client, url: &str, payload: &Submission) -> bool {
    match client.post(url).json(payload).send().await {
        Ok(response) => response.status().is_server_error(),
        Err(_) => true,
    }
}

pub(super) fn schedule(app: tauri::AppHandle, state: SharingState) {
    {
        let Ok(mut inner) = state.inner.lock() else {
            return;
        };
        if inner.running {
            return;
        }
        inner.running = true;
    }
    tauri::async_runtime::spawn(async move {
        let Ok(client) = client(Duration::from_secs(10)) else {
            if let Ok(mut inner) = state.inner.lock() {
                inner.running = false;
            }
            return;
        };
        loop {
            if let Some(item) = state.next(&app.state::<AppConfigState>(), now_seconds()) {
                let retryable = send(&client, URL, &item.payload).await;
                state.sent(&item, retryable, now_seconds());
            } else if let Some(delay) = state.next_delay(now_seconds()) {
                tokio::time::sleep(Duration::from_secs(delay.max(1))).await;
            } else {
                break;
            }
        }
    });
}

//! 本进程安全报告快照和用户确认发送；不接受任意日志正文。
use crate::failure_diagnostic::SafeReport;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeSet, VecDeque};
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const REPORT_URL: &str = "https://mocika-shield-stats-api.xuechao-suo.workers.dev/reports/errors";
const MAX_REPORTS: usize = 5;
const TTL_SECONDS: u64 = 30 * 60;

#[derive(Clone, Serialize)]
pub(crate) struct ReportPreview {
    pub(crate) report_id: String,
    pub(crate) digest: String,
    pub(crate) payload_json: String,
    pub(crate) should_prompt: bool,
    pub(crate) sent: bool,
}
struct CachedReport {
    preview: ReportPreview,
    created_at: u64,
    sending: bool,
}

#[derive(Default)]
struct ReportCache {
    entries: VecDeque<CachedReport>,
    prompted: BTreeSet<String>,
    attempted: BTreeSet<String>,
}
impl ReportCache {
    fn prune(&mut self, now: u64) {
        self.entries
            .retain(|entry| entry.sending || now.saturating_sub(entry.created_at) < TTL_SECONDS);
    }
    fn insert(
        &mut self,
        report: SafeReport,
        prompt: bool,
        now: u64,
    ) -> Result<ReportPreview, String> {
        self.prune(now);
        let json = serde_json::to_string(&report).map_err(|_| "无法生成安全报告".to_string())?;
        if json.len() > 8192 {
            return Err("安全报告超过大小限制".into());
        }
        let category = format!(
            "{:?}|{:?}|{}",
            report.context, report.code, report.app_version
        );
        // 枚举空间有限，但也约束会话提示集合，避免长时间运行无限增长。
        let should_prompt = prompt && self.prompted.len() < 1024 && self.prompted.insert(category);
        let preview = ReportPreview {
            report_id: report.report_id,
            digest: format!("{:x}", Sha256::digest(json.as_bytes())),
            payload_json: json,
            should_prompt,
            sent: false,
        };
        if self.entries.len() >= MAX_REPORTS {
            let Some(index) = self.entries.iter().position(|entry| !entry.sending) else {
                return Err("报告正在发送，请稍后再试".into());
            };
            self.entries.remove(index);
        }
        self.entries.push_back(CachedReport {
            preview: preview.clone(),
            created_at: now,
            sending: false,
        });
        Ok(preview)
    }
    fn preview(&mut self, id: &str, now: u64) -> Result<ReportPreview, String> {
        self.prune(now);
        self.entries
            .iter()
            .find(|entry| entry.preview.report_id == id)
            .map(|entry| entry.preview.clone())
            .ok_or_else(|| "报告已过期或已被替换，请重新执行操作".into())
    }
    fn latest(&mut self, now: u64) -> Option<ReportPreview> {
        self.prune(now);
        self.entries.back().map(|entry| {
            let mut preview = entry.preview.clone();
            preview.should_prompt = false;
            preview
        })
    }
    fn begin_send(&mut self, id: &str, digest: &str, now: u64) -> Result<String, String> {
        self.prune(now);
        let entry = self
            .entries
            .iter_mut()
            .find(|entry| entry.preview.report_id == id)
            .ok_or_else(|| "报告已过期，请重新执行操作".to_string())?;
        if entry.preview.digest != digest {
            return Err("报告预览已变化，请重新查看后确认".into());
        }
        if entry.sending {
            return Err("报告正在发送，请勿重复操作".into());
        }
        if entry.preview.sent {
            return Err("此报告已发送".into());
        }
        if !self.attempted.contains(id) && self.attempted.len() >= MAX_REPORTS {
            return Err("本次应用会话已达到 5 份报告上限".into());
        }
        self.attempted.insert(id.to_string());
        entry.sending = true;
        Ok(entry.preview.payload_json.clone())
    }
    fn finish_send(&mut self, id: &str, success: bool) {
        if let Some(entry) = self
            .entries
            .iter_mut()
            .find(|entry| entry.preview.report_id == id)
        {
            entry.sending = false;
            entry.preview.sent |= success;
        }
    }
}

#[derive(Default)]
pub(crate) struct ErrorReportState(Mutex<ReportCache>);

pub(crate) fn now_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

impl ErrorReportState {
    pub(crate) fn capture(
        &self,
        report: SafeReport,
        prompt: bool,
    ) -> Result<ReportPreview, String> {
        self.0
            .lock()
            .map_err(|_| "报告状态不可用".to_string())?
            .insert(report, prompt, now_seconds())
    }
}

#[tauri::command]
pub(crate) fn preview_error_report(
    state: tauri::State<'_, ErrorReportState>,
    report_id: String,
) -> Result<ReportPreview, String> {
    state
        .0
        .lock()
        .map_err(|_| "报告状态不可用".to_string())?
        .preview(&report_id, now_seconds())
}

#[tauri::command]
pub(crate) fn latest_error_report(
    state: tauri::State<'_, ErrorReportState>,
) -> Result<Option<ReportPreview>, String> {
    Ok(state
        .0
        .lock()
        .map_err(|_| "报告状态不可用".to_string())?
        .latest(now_seconds()))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Receipt {
    report_id: String,
}

#[tauri::command]
pub(crate) async fn send_error_report(
    state: tauri::State<'_, ErrorReportState>,
    report_id: String,
    digest: String,
) -> Result<String, String> {
    let payload = state
        .0
        .lock()
        .map_err(|_| "报告状态不可用".to_string())?
        .begin_send(&report_id, &digest, now_seconds())?;
    let result = send_payload(payload, &report_id).await;
    if let Ok(mut cache) = state.0.lock() {
        cache.finish_send(&report_id, result.is_ok());
    }
    result.map(|()| report_id)
}

async fn send_payload(payload: String, report_id: &str) -> Result<(), String> {
    send_payload_to(REPORT_URL, payload, report_id).await
}

async fn send_payload_to(url: &str, payload: String, report_id: &str) -> Result<(), String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|_| "无法初始化报告连接".to_string())?;
    let response = client
        .post(url)
        .header("Content-Type", "application/json")
        .body(payload)
        .send()
        .await
        .map_err(|_| "报告发送失败或超时，可手动重试；不影响本地操作".to_string())?;
    match response.status().as_u16() {
        200 | 201 => {
            // 有界读取回执，绝不把远端正文作为界面错误展示。
            let mut response = response;
            let mut bytes = Vec::new();
            while let Some(chunk) = response
                .chunk()
                .await
                .map_err(|_| "无法读取报告回执".to_string())?
            {
                if bytes.len() + chunk.len() > 1024 {
                    return Err("报告回执格式不符合约定".into());
                }
                bytes.extend_from_slice(&chunk);
            }
            let receipt: Receipt =
                serde_json::from_slice(&bytes).map_err(|_| "报告回执格式不符合约定".to_string())?;
            if receipt.report_id != report_id {
                return Err("报告编号不匹配，请稍后重试".into());
            }
            Ok(())
        }
        409 => Err("报告编号发生冲突，请重新执行操作生成报告".into()),
        429 => Err("报告接收已达到限额，请稍后手动重试".into()),
        503 => Err("错误报告服务暂未开放或暂时不可用，不影响本地操作".into()),
        _ => Err("报告未被接收，请稍后重试或通过讨论区反馈".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::failure_diagnostic::FailureContext;
    use shield_core::diagnostic::{Diagnostic, FailureCode};

    #[test]
    fn 实际请求字节等于预览且只接受对应回执() {
        use std::io::{Read, Write};
        let server = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = server.local_addr().unwrap();
        let mut cache = ReportCache::default();
        let view = cache.insert(report(10), true, 10).unwrap();
        let expected = view.payload_json.clone();
        let id = view.report_id.clone();
        let handle = std::thread::spawn(move || {
            let (mut socket, _) = server.accept().unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut bytes = Vec::new();
            let body_start = loop {
                let mut chunk = [0u8; 1024];
                let size = socket.read(&mut chunk).unwrap();
                assert!(size > 0);
                bytes.extend_from_slice(&chunk[..size]);
                if let Some(index) = bytes.windows(4).position(|value| value == b"\r\n\r\n") {
                    break index + 4;
                }
            };
            while bytes.len() - body_start < expected.len() {
                let mut chunk = [0u8; 1024];
                let size = socket.read(&mut chunk).unwrap();
                assert!(size > 0);
                bytes.extend_from_slice(&chunk[..size]);
            }
            assert_eq!(&bytes[body_start..], expected.as_bytes());
            let receipt = serde_json::json!({"report_id": id}).to_string();
            write!(
                socket,
                "HTTP/1.1 201 Created\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                receipt.len(),
                receipt
            )
            .unwrap();
        });
        let payload = cache.begin_send(&view.report_id, &view.digest, 11).unwrap();
        tauri::async_runtime::block_on(send_payload_to(
            &format!("http://{address}/reports/errors"),
            payload,
            &view.report_id,
        ))
        .unwrap();
        handle.join().unwrap();
    }

    fn report(now: u64) -> SafeReport {
        SafeReport::new(
            FailureContext::sign("SignApk"),
            Diagnostic::new(FailureCode::SigningFailed),
            now,
        )
    }

    #[test]
    fn 预览摘要绑定原始载荷且重复发送被拒绝() {
        let mut cache = ReportCache::default();
        let view = cache.insert(report(10), true, 10).unwrap();
        assert!(cache.begin_send(&view.report_id, "伪造摘要", 11).is_err());
        let json = cache.begin_send(&view.report_id, &view.digest, 11).unwrap();
        assert_eq!(json, view.payload_json);
        assert!(cache.begin_send(&view.report_id, &view.digest, 11).is_err());
        cache.finish_send(&view.report_id, false);
        assert_eq!(
            cache.begin_send(&view.report_id, &view.digest, 12).unwrap(),
            json
        );
        cache.finish_send(&view.report_id, true);
        assert!(cache.begin_send(&view.report_id, &view.digest, 13).is_err());
    }

    #[test]
    fn 快照有容量和有效期且同类仅提示一次() {
        let mut cache = ReportCache::default();
        let first = cache.insert(report(10), true, 10).unwrap();
        assert!(first.should_prompt);
        for time in 11..17 {
            assert!(
                !cache
                    .insert(report(time), true, time)
                    .unwrap()
                    .should_prompt
            );
        }
        assert_eq!(cache.entries.len(), 5);
        assert!(cache.preview(&first.report_id, 17).is_err());
        assert!(cache.latest(2000).is_none());
    }

    #[test]
    fn 取消仅关闭本地预览不会触发发送() {
        let mut cache = ReportCache::default();
        let view = cache.insert(report(10), false, 10).unwrap();
        assert!(!view.should_prompt);
        assert_eq!(cache.attempted.len(), 0);
        assert_eq!(
            cache.preview(&view.report_id, 11).unwrap().payload_json,
            view.payload_json
        );
    }
}

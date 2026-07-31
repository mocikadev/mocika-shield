use serde::Serialize;

#[derive(Debug, Serialize)]
pub(crate) struct DoneEvent {
    #[serde(rename = "type")]
    kind: &'static str,
}

#[derive(Debug, Serialize)]
pub(crate) struct ProgressEvent {
    #[serde(rename = "type")]
    kind: &'static str,
    step: String,
    message: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct ErrorEvent {
    #[serde(rename = "type")]
    kind: &'static str,
    message: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct ApkCheckOutput {
    pub already_protected: bool,
    pub is_signed: bool,
    pub cert_fingerprint: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Serialize)]
pub(crate) struct KeystoreCheckOutput {
    pub cert_fingerprint: Option<String>,
    pub error: Option<String>,
}

pub(crate) fn done_event_json() -> String {
    serde_json::to_string(&DoneEvent { kind: "done" }).expect("序列化 done 事件失败")
}

pub(crate) fn progress_event_json(step: &str, message: &str) -> String {
    serde_json::to_string(&ProgressEvent {
        kind: "progress",
        step: step.to_string(),
        message: message.to_string(),
    })
    .expect("序列化进度事件失败")
}

pub(crate) fn error_event_json(message: String) -> String {
    serde_json::to_string(&ErrorEvent {
        kind: "error",
        message,
    })
    .expect("序列化 error 事件失败")
}

pub(crate) fn apk_check_json(
    already_protected: bool,
    is_signed: bool,
    cert_fingerprint: Option<String>,
) -> String {
    serde_json::to_string(&ApkCheckOutput {
        already_protected,
        is_signed,
        cert_fingerprint,
        error: None,
    })
    .expect("序列化 APK 检查结果失败")
}

pub(crate) fn keystore_check_json(cert_fingerprint: String) -> String {
    serde_json::to_string(&KeystoreCheckOutput {
        cert_fingerprint: Some(cert_fingerprint),
        error: None,
    })
    .expect("序列化 keystore 检查结果失败")
}

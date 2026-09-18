//! 唯一上传模型；本地路径、任务和安装标识不得进入此边界。
use crate::application_identity::ApplicationIdentity;

pub(super) fn now_seconds() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[derive(Clone, serde::Serialize)]
pub(super) struct Submission {
    pub(super) submission_id: String,
    schema_version: u8,
    notice_version: u8,
    pub(super) package_name: String,
    app_name: Option<String>,
    app_version_code: Option<String>,
    tool_version: String,
    operation: String,
    flow: String,
    success_date: String,
}

impl Submission {
    pub(super) fn new(
        identity: ApplicationIdentity,
        flow: &str,
        success_date: String,
        tool_version: String,
    ) -> Self {
        Self {
            submission_id: uuid::Uuid::new_v4().to_string(),
            schema_version: 1,
            notice_version: 1,
            package_name: identity.package_name,
            app_name: identity.app_name,
            app_version_code: identity.app_version_code,
            tool_version,
            operation: if flow == "sign" { "sign" } else { "protect" }.into(),
            flow: flow.into(),
            success_date,
        }
    }
}

pub(super) fn utc_date(seconds: u64) -> String {
    let z = (seconds / 86_400) as i64 + 719_468;
    let era = z / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = mp + if mp < 10 { 3 } else { -9 };
    format!("{:04}-{m:02}-{d:02}", y + i64::from(m <= 2))
}

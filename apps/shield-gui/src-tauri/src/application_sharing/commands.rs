use super::{protocol::now_seconds, SharingState};
use crate::app_config::AppConfigState;
use tauri::{Emitter, Manager};

#[derive(serde::Serialize, Clone)]
pub(crate) struct InspectionPreview {
    inspection_id: String,
    package_name: String,
    enabled: bool,
}

#[derive(serde::Serialize, Clone)]
struct PreferenceChanged {
    package_name: String,
    enabled: bool,
}

#[tauri::command]
pub(crate) async fn inspect_application_sharing(
    app: tauri::AppHandle,
    path: String,
) -> Result<Option<InspectionPreview>, String> {
    tokio::task::spawn_blocking(move || {
        let path = std::path::PathBuf::from(path);
        let Some((file, identity)) = super::inspection::InspectionStore::read_input(&path) else {
            return Ok(None);
        };
        let state = app.state::<SharingState>();
        let mut store = state
            .inspections
            .lock()
            .map_err(|_| "应用分享状态不可用".to_string())?;
        let id = store.insert(path, file, identity, now_seconds(), |package| {
            state.suppress(&app.state::<AppConfigState>(), package);
            let _ = app.emit(
                "application-sharing-changed",
                PreferenceChanged {
                    package_name: package.to_string(),
                    enabled: false,
                },
            );
        });
        let Some(package) = store.package(&id, now_seconds()).map(str::to_string) else {
            return Ok(None);
        };
        let enabled = state
            .authorization(&app.state::<AppConfigState>(), &package)
            .is_some();
        Ok(Some(InspectionPreview {
            inspection_id: id,
            package_name: package,
            enabled,
        }))
    })
    .await
    .map_err(|_| "应用身份检查失败".to_string())?
}

#[tauri::command]
pub(crate) fn release_application_inspection(
    state: tauri::State<'_, SharingState>,
    inspection_id: String,
) {
    if let Ok(mut store) = state.inspections.lock() {
        store.release(&inspection_id);
    }
}

#[tauri::command]
pub(crate) async fn save_application_sharing(
    app: tauri::AppHandle,
    inspection_id: String,
    enabled: bool,
) -> Result<(), String> {
    tokio::task::spawn_blocking(move || {
        let state = app.state::<SharingState>();
        let mut store = state
            .inspections
            .lock()
            .map_err(|_| "应用分享状态不可用".to_string())?;
        let package = store
            .preference_package(&inspection_id, now_seconds(), enabled)
            .ok_or_else(|| "应用身份检查已失效，请重新选择 APK".to_string())?;
        let result = state.preference(&app.state::<AppConfigState>(), &package, enabled);
        let _ = app.emit(
            "application-sharing-changed",
            PreferenceChanged {
                package_name: package,
                enabled: enabled && result.is_ok(),
            },
        );
        result
    })
    .await
    .map_err(|_| "应用分享偏好保存失败".to_string())?
}

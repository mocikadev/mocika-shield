use crate::app_paths::{find_apktool_path, find_apksigner_path};
use serde::Serialize;
use shield_cli::utils::no_window_command;

#[derive(Serialize)]
pub(crate) struct BuildInfo {
    pub apktool_version: String,
    pub apksigner_version: String,
}

#[derive(Serialize)]
pub(crate) struct AppInfo {
    pub version: String,
    pub git_hash: String,
    pub build_date: String,
}

pub(crate) fn get_app_info() -> AppInfo {
    AppInfo {
        version: env!("CARGO_PKG_VERSION").to_string(),
        git_hash: option_env!("GIT_HASH").unwrap_or("dev").to_string(),
        build_date: option_env!("BUILD_DATE").unwrap_or("unknown").to_string(),
    }
}

pub(crate) fn get_build_info(app: tauri::AppHandle) -> BuildInfo {
    let apktool_version = find_apktool_path(&app)
        .and_then(|jar| {
            let path_str = jar.to_str()?.to_string();
            no_window_command("java")
                .args(["-jar", &path_str, "--version"])
                .output()
                .ok()
                .filter(|o| o.status.success())
                .map(|o| {
                    String::from_utf8_lossy(&o.stdout)
                        .lines()
                        .next()
                        .unwrap_or("")
                        .trim()
                        .to_string()
                })
        })
        .unwrap_or_else(|| "未找到".to_string());

    let apksigner_version = find_apksigner_path(&app)
        .and_then(|jar| {
            let path_str = jar.to_str()?.to_string();
            no_window_command("java")
                .args(["-jar", &path_str, "--version"])
                .output()
                .ok()
                .filter(|o| o.status.success())
                .map(|o| {
                    String::from_utf8_lossy(&o.stdout)
                        .lines()
                        .next()
                        .unwrap_or("")
                        .trim()
                        .to_string()
                })
        })
        .unwrap_or_else(|| "未找到".to_string());

    BuildInfo {
        apktool_version,
        apksigner_version,
    }
}

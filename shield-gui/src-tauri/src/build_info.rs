use crate::app_paths::{find_apktool_path, find_apksigner_path};
use serde::Serialize;
use shield_cli::utils::{no_window_command, probe_java_environment, MIN_JAVA_MAJOR_VERSION};

#[derive(Serialize)]
pub(crate) struct BuildInfo {
    pub apktool_version: String,
    pub apksigner_version: String,
    pub java_version: String,
    pub java_ready: bool,
    pub keytool_ready: bool,
    pub javac_ready: bool,
    pub java_major: Option<u32>,
    pub min_java_major: u32,
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
    let java_info = probe_java_environment();
    let java_version = java_info.version_label().to_string();
    let java_ready = java_info.java_ready();
    let java_path = java_info.java_path.clone();

    let apktool_version = find_apktool_path(&app)
        .and_then(|jar| {
            let java = java_path.as_ref()?;
            let path_str = jar.to_str()?.to_string();
            no_window_command(java)
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
        .unwrap_or_else(|| {
            if java_ready {
                "未找到".to_string()
            } else {
                format!("需要 Java {}+", MIN_JAVA_MAJOR_VERSION)
            }
        });

    let apksigner_version = find_apksigner_path(&app)
        .and_then(|jar| {
            let java = java_path.as_ref()?;
            let path_str = jar.to_str()?.to_string();
            no_window_command(java)
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
        .unwrap_or_else(|| {
            if java_ready {
                "未找到".to_string()
            } else {
                format!("需要 Java {}+", MIN_JAVA_MAJOR_VERSION)
            }
        });

    BuildInfo {
        apktool_version,
        apksigner_version,
        java_version,
        java_ready,
        keytool_ready: java_info.keytool_ready(),
        javac_ready: java_info.javac_ready(),
        java_major: java_info.major_version,
        min_java_major: MIN_JAVA_MAJOR_VERSION,
    }
}

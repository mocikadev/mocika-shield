use crate::app_paths::{find_apksigner_path, find_apktool_path};
use serde::Serialize;
use shield_core::utils::{no_window_command, probe_java_environment, MIN_JAVA_MAJOR_VERSION};
use std::fs;
use tauri::Manager;

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

pub(crate) fn get_diagnostic_info(app: tauri::AppHandle) -> String {
    let app_info = get_app_info();
    let build_info = get_build_info(app.clone());
    let config_dir_status = dir_status(app.path().app_config_dir().ok());
    let data_dir_status = dir_status(app.path().app_data_dir().ok());
    let apktool_status = tool_status(find_apktool_path(&app));
    let apksigner_status = tool_status(find_apksigner_path(&app));

    [
        "Mocika Shield 诊断信息".to_string(),
        format!("版本: {}", app_info.version),
        format!("Git: {}", app_info.git_hash),
        format!("构建日期: {}", app_info.build_date),
        format!(
            "平台: {} / {}",
            std::env::consts::OS,
            std::env::consts::ARCH
        ),
        format!("Java: {}", build_info.java_version),
        format!("Java 就绪: {}", yes_no(build_info.java_ready)),
        format!("keytool 就绪: {}", yes_no(build_info.keytool_ready)),
        format!("javac 就绪: {}", yes_no(build_info.javac_ready)),
        format!("最低 Java 要求: {}", build_info.min_java_major),
        format!("apktool: {}", build_info.apktool_version),
        format!("apktool 文件: {apktool_status}"),
        format!("apksigner: {}", build_info.apksigner_version),
        format!("apksigner 文件: {apksigner_status}"),
        format!("配置目录: {config_dir_status}"),
        format!("数据目录: {data_dir_status}"),
        "说明: 诊断信息不包含 APK 路径、证书路径、密码或完整用户目录。".to_string(),
    ]
    .join("\n")
}

fn yes_no(value: bool) -> &'static str {
    if value {
        "是"
    } else {
        "否"
    }
}

fn tool_status(path: Option<std::path::PathBuf>) -> &'static str {
    if path.as_deref().is_some_and(|path| path.exists()) {
        "已找到"
    } else {
        "未找到"
    }
}

fn dir_status(path: Option<std::path::PathBuf>) -> String {
    let Some(path) = path else {
        return "无法定位".to_string();
    };

    match fs::metadata(&path) {
        Ok(meta) if meta.is_dir() => match fs::write(path.join(".mocika-shield-diagnostic"), b"ok")
        {
            Ok(()) => {
                let _ = fs::remove_file(path.join(".mocika-shield-diagnostic"));
                "可读写".to_string()
            }
            Err(err) => format!("不可写: {err}"),
        },
        Ok(_) => "路径不是目录".to_string(),
        Err(err) => format!("不可用: {err}"),
    }
}

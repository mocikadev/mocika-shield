use std::path::{Path, PathBuf};
use tauri::Manager;

pub(crate) fn application_paths(
    app: &tauri::AppHandle,
) -> Result<shield_services::paths::ApplicationPaths, String> {
    let actual = shield_services::paths::ApplicationPaths::from_directories(
        app.path()
            .app_config_dir()
            .map_err(|_| "无法定位应用配置目录".to_string())?,
        app.path()
            .app_data_dir()
            .map_err(|_| "无法定位应用数据目录".to_string())?,
    )
    .map_err(str::to_owned)?;
    let shared = shield_services::paths::ApplicationPaths::current_user().map_err(str::to_owned)?;
    if actual != shared {
        return Err("共享应用目录与桌面配置不一致，请检查应用标识".into());
    }
    Ok(shared)
}

pub(crate) fn strip_unc_prefix(path: PathBuf) -> PathBuf {
    dunce::simplified(&path).to_path_buf()
}

fn resource_dir(app: &tauri::AppHandle) -> Option<PathBuf> {
    app.path().resource_dir().ok().map(strip_unc_prefix)
}

fn appimage_resource_dir() -> Option<PathBuf> {
    let appdir = std::env::var("APPDIR").ok()?;
    Some(PathBuf::from(appdir).join("usr/lib/mocika-shield"))
}

pub(crate) fn find_apktool_path(app: &tauri::AppHandle) -> Option<PathBuf> {
    for base in resource_dir(app).into_iter().chain(appimage_resource_dir()) {
        let p = base.join("tools/apktool.jar");
        if p.exists() {
            return Some(p);
        }
    }
    let dev = project_root_path().join("tools/apktool_3.0.1.jar");
    if dev.exists() {
        return Some(dev);
    }
    None
}

pub(crate) fn find_resources_path(app: &tauri::AppHandle) -> Option<PathBuf> {
    find_named_resources_path(app, "resources.zip")
}

pub(crate) fn find_named_resources_path(
    app: &tauri::AppHandle,
    file_name: &str,
) -> Option<PathBuf> {
    for base in resource_dir(app).into_iter().chain(appimage_resource_dir()) {
        let p = base.join("resources").join(file_name);
        if p.exists() {
            return Some(p);
        }
    }
    let dev = project_root_path()
        .join("shield-stub/build/outputs/resources")
        .join(file_name);
    if dev.exists() {
        return Some(dev);
    }
    None
}

pub(crate) fn find_apksigner_path(app: &tauri::AppHandle) -> Option<PathBuf> {
    for base in resource_dir(app).into_iter().chain(appimage_resource_dir()) {
        let p = base.join("tools/apksigner.jar");
        if p.exists() {
            return Some(p);
        }
    }
    let dev = project_root_path().join("tools/apksigner.jar");
    if dev.exists() {
        return Some(dev);
    }
    None
}

pub(crate) fn project_root_path() -> PathBuf {
    let current = strip_unc_prefix(std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
    let mut path = current.clone();
    loop {
        if path.join("shield-stub").exists() && path.join("apps").exists() {
            return path;
        }
        if !path.pop() {
            break;
        }
    }
    current
}

pub(crate) fn parent_dir_string(path: &str) -> String {
    Path::new(path)
        .parent()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|| path.to_string())
}

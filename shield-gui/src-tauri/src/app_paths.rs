use std::path::{Path, PathBuf};
use tauri::Manager;

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
    for base in resource_dir(app).into_iter().chain(appimage_resource_dir()) {
        let p = base.join("resources/resources.zip");
        if p.exists() {
            return Some(p);
        }
    }
    let dev = project_root_path().join("shield-stub/build/outputs/resources/resources.zip");
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
        if path.join("shield-stub").exists() && path.join("shield-cli").exists() {
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

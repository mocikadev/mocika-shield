use crate::app_paths::parent_dir_string;
use std::path::PathBuf;

pub(crate) fn show_in_folder(path: String) -> Result<(), String> {
    let dir = parent_dir_string(&path);
    #[cfg(target_os = "linux")]
    std::process::Command::new("xdg-open")
        .arg(&dir)
        .spawn()
        .map_err(|e| e.to_string())?;
    #[cfg(target_os = "macos")]
    std::process::Command::new("open")
        .arg(&dir)
        .spawn()
        .map_err(|e| e.to_string())?;
    #[cfg(target_os = "windows")]
    std::process::Command::new("explorer")
        .arg(&dir)
        .spawn()
        .map_err(|e| e.to_string())?;
    Ok(())
}

pub(crate) fn check_file_exists(path: String) -> bool {
    PathBuf::from(path).exists()
}

pub(crate) fn delete_file(path: String) -> Result<(), String> {
    let p = PathBuf::from(&path);
    if p.exists() {
        std::fs::remove_file(&p).map_err(|e| format!("删除文件失败: {e}"))?;
    }
    Ok(())
}

pub(crate) fn open_url(url: String) -> Result<(), String> {
    #[cfg(target_os = "linux")]
    std::process::Command::new("xdg-open")
        .arg(&url)
        .spawn()
        .map_err(|e| e.to_string())?;
    #[cfg(target_os = "macos")]
    std::process::Command::new("open")
        .arg(&url)
        .spawn()
        .map_err(|e| e.to_string())?;
    #[cfg(target_os = "windows")]
    shield_core::utils::no_window_command("cmd")
        .args(["/c", "start", "", &url])
        .spawn()
        .map_err(|e| e.to_string())?;
    Ok(())
}

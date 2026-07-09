use crate::app_paths::parent_dir_string;
use std::path::{Path, PathBuf};

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
    ensure_deletable_file(&p)?;
    if p.exists() {
        std::fs::remove_file(&p).map_err(|e| format!("删除文件失败: {e}"))?;
    }
    Ok(())
}

pub(crate) fn open_url(url: String) -> Result<(), String> {
    ensure_allowed_url(&url)?;
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

fn ensure_deletable_file(path: &Path) -> Result<(), String> {
    if path
        .extension()
        .and_then(|value| value.to_str())
        .map(|value| matches!(value.to_ascii_lowercase().as_str(), "apk" | "idsig"))
        .unwrap_or(false)
    {
        return Ok(());
    }

    Err("只允许删除 APK 或 idsig 文件".to_string())
}

fn ensure_allowed_url(url: &str) -> Result<(), String> {
    let trimmed = url.trim();
    let Some(rest) = trimmed.strip_prefix("https://github.com/") else {
        return Err("只允许打开 GitHub HTTPS 链接".to_string());
    };

    if rest == "mocikadev/mocika-shield" || rest.starts_with("mocikadev/mocika-shield/") {
        Ok(())
    } else {
        Err("只允许打开 Mocika Shield 仓库相关链接".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 只允许删除_apk_和_idsig() {
        assert!(ensure_deletable_file(Path::new("/tmp/app.apk")).is_ok());
        assert!(ensure_deletable_file(Path::new("/tmp/app.apk.idsig")).is_ok());
        assert!(ensure_deletable_file(Path::new("/tmp/config.toml")).is_err());
    }

    #[test]
    fn 只允许打开项目_github_链接() {
        assert!(ensure_allowed_url("https://github.com/mocikadev/mocika-shield/releases").is_ok());
        assert!(ensure_allowed_url("http://github.com/mocikadev/mocika-shield").is_err());
        assert!(ensure_allowed_url("https://github.com/other/repo").is_err());
    }
}

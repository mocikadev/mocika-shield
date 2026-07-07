use anyhow::{Context, Result};
use colored::Colorize;
use directories::ProjectDirs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};

static JSON_MODE: AtomicBool = AtomicBool::new(false);

pub fn set_json_mode(enabled: bool) {
    JSON_MODE.store(enabled, Ordering::SeqCst);
}

pub fn is_json_mode() -> bool {
    JSON_MODE.load(Ordering::Relaxed)
}

pub fn run_command(cmd: &str, args: &[&str], cwd: Option<&Path>) -> Result<String> {
    log::debug!("执行命令: {} {}", cmd, args.join(" "));

    let mut command = Command::new(cmd);
    command
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    if let Some(dir) = cwd {
        command.current_dir(dir);
    }

    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        command.creation_flags(CREATE_NO_WINDOW);
    }

    let output = command
        .output()
        .with_context(|| format!("执行命令失败: {}", cmd))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("命令执行失败: {}\n错误: {}", cmd, stderr);
    }

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    Ok(stdout)
}

#[allow(dead_code)]
pub fn check_tool(name: &str) -> Result<()> {
    which::which(name).with_context(|| format!("未找到工具: {}", name))?;
    Ok(())
}

pub fn print_step(step: &str) {
    if !is_json_mode() {
        println!("\n{} {}", "=>".cyan().bold(), step.bold());
    }
}

pub fn print_success(msg: &str) {
    if !is_json_mode() {
        println!("{} {}", "✓".green().bold(), msg);
    }
}

#[allow(dead_code)]
pub fn print_error(msg: &str) {
    if !is_json_mode() {
        eprintln!("{} {}", "✗".red().bold(), msg);
    }
}

#[allow(dead_code)]
pub fn print_warning(msg: &str) {
    if !is_json_mode() {
        println!("{} {}", "!".yellow().bold(), msg);
    }
}

pub fn human_size(bytes: u64) -> String {
    const UNITS: &[&str] = &["B", "KB", "MB", "GB"];
    let mut size = bytes as f64;
    let mut unit_index = 0;

    while size >= 1024.0 && unit_index < UNITS.len() - 1 {
        size /= 1024.0;
        unit_index += 1;
    }

    format!("{:.1}{}", size, UNITS[unit_index])
}

pub fn create_temp_dir(prefix: &str) -> Result<tempfile::TempDir> {
    tempfile::Builder::new()
        .prefix(prefix)
        .tempdir()
        .context("创建临时目录失败")
}

pub fn exe_dir() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| strip_unc_prefix(p)))
        .unwrap_or_else(|| PathBuf::from("."))
}

pub fn strip_unc_prefix(path: &Path) -> PathBuf {
    dunce::simplified(path).to_path_buf()
}

pub fn dev_project_root() -> PathBuf {
    let current = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
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

fn system_data_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();

    #[cfg(target_os = "linux")]
    {
        dirs.push(PathBuf::from("/usr/local/share/mocika-shield"));
        dirs.push(PathBuf::from("/usr/share/mocika-shield"));
    }

    #[cfg(target_os = "macos")]
    {
        dirs.push(PathBuf::from("/Library/Application Support/mocika-shield"));
    }

    #[cfg(target_os = "windows")]
    {
        if let Ok(appdata) = std::env::var("ProgramData") {
            dirs.push(PathBuf::from(appdata).join("mocika-shield"));
        }
    }

    dirs
}

fn data_search_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(proj) = ProjectDirs::from("dev", "mocika", "mocika-shield") {
        dirs.push(strip_unc_prefix(proj.data_local_dir()));
        dirs.push(strip_unc_prefix(proj.data_dir()));
    }
    dirs.extend(system_data_dirs());
    dirs
}

fn user_data_hint() -> &'static str {
    #[cfg(target_os = "linux")]
    return "~/.local/share/mocika-shield/";
    #[cfg(target_os = "macos")]
    return "~/Library/Application Support/mocika-shield/";
    #[cfg(target_os = "windows")]
    return "%APPDATA%\\mocika\\mocika-shield\\data\\";
    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    return "<用户数据目录>/mocika-shield/";
}

fn system_data_hint() -> &'static str {
    #[cfg(target_os = "linux")]
    return "/usr/local/share/mocika-shield/";
    #[cfg(target_os = "macos")]
    return "/Library/Application Support/mocika-shield/";
    #[cfg(target_os = "windows")]
    return "%ProgramData%\\mocika-shield\\";
    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    return "<系统数据目录>/mocika-shield/";
}

pub fn find_java() -> Result<PathBuf> {
    if let Ok(p) = which::which("java") {
        return Ok(p);
    }
    anyhow::bail!(
        "未找到 Java 运行环境（java），请先安装 JDK/JRE 8 或以上版本。\n\
         - macOS：brew install --cask temurin\n\
         - 或从 https://adoptium.net 下载安装"
    )
}

pub fn find_apktool() -> Result<PathBuf> {
    let exe = exe_dir();

    let candidates = [
        exe.join("../lib/apktool.jar"),
        exe.join("../Resources/tools/apktool.jar"),
    ];
    for p in &candidates {
        let resolved = strip_unc_prefix(p);
        if resolved.exists() {
            return Ok(resolved);
        }
    }

    for data_dir in data_search_dirs() {
        let p = data_dir.join("lib/apktool.jar");
        if p.exists() {
            return Ok(p);
        }
    }

    let dev = dev_project_root().join("tools/apktool_3.0.1.jar");
    if dev.exists() {
        return Ok(dev);
    }

    anyhow::bail!(
        "未找到 apktool.jar，可将其放到以下任一位置：\n\
         - 发布包：<shield 所在目录>/../lib/apktool.jar\n\
         - 用户数据：{}lib/apktool.jar\n\
         - 系统数据：{}lib/apktool.jar",
        user_data_hint(),
        system_data_hint(),
    )
}

pub fn find_apksigner() -> Result<PathBuf> {
    let exe = exe_dir();

    let candidates = [
        exe.join("../lib/apksigner.jar"),
        exe.join("../Resources/tools/apksigner.jar"),
    ];
    for p in &candidates {
        let resolved = strip_unc_prefix(p);
        if resolved.exists() {
            return Ok(resolved);
        }
    }

    for data_dir in data_search_dirs() {
        let p = data_dir.join("lib/apksigner.jar");
        if p.exists() {
            return Ok(p);
        }
    }

    let dev = dev_project_root().join("tools/apksigner.jar");
    if dev.exists() {
        return Ok(dev);
    }

    anyhow::bail!(
        "未找到 apksigner.jar，可将其放到以下任一位置：\n\
         - 发布包：<shield 所在目录>/../lib/apksigner.jar\n\
         - 用户数据：{}lib/apksigner.jar\n\
         - 系统数据：{}lib/apksigner.jar\n\
         - GUI 用户：请在设置页面手动指定路径",
        user_data_hint(),
        system_data_hint(),
    )
}

pub fn find_runtime_resources() -> Result<PathBuf> {
    let exe = exe_dir();

    let candidates = [
        exe.join("../resources/resources.zip"),
        exe.join("../resources/resources/resources.zip"),
    ];
    for p in &candidates {
        if p.exists() {
            return Ok(p.clone());
        }
    }

    for data_dir in data_search_dirs() {
        let p = data_dir.join("resources/resources.zip");
        if p.exists() {
            return Ok(p);
        }
    }

    let dev = dev_project_root().join("shield-stub/build/outputs/resources/resources.zip");
    if dev.exists() {
        return Ok(dev);
    }

    anyhow::bail!(
        "未找到 resources.zip，可将其放到以下任一位置：\n\
         - 发布包：<shield 所在目录>/../resources/resources.zip\n\
         - 用户数据：{}resources/resources.zip\n\
         - 系统数据：{}resources/resources.zip\n\
         - 从源码构建：make build-stub",
        user_data_hint(),
        system_data_hint(),
    )
}

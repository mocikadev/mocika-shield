use anyhow::{Context, Result};
use colored::Colorize;
use directories::ProjectDirs;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};

static JSON_MODE: AtomicBool = AtomicBool::new(false);
pub const MIN_JAVA_MAJOR_VERSION: u32 = 17;

#[derive(Debug, Clone)]
pub struct JavaEnvironmentInfo {
    pub java_path: Option<PathBuf>,
    pub keytool_path: Option<PathBuf>,
    pub javac_path: Option<PathBuf>,
    pub version_text: Option<String>,
    pub major_version: Option<u32>,
}

impl JavaEnvironmentInfo {
    pub fn java_ready(&self) -> bool {
        self.java_path.is_some() && self.major_version.unwrap_or(0) >= MIN_JAVA_MAJOR_VERSION
    }

    pub fn keytool_ready(&self) -> bool {
        self.keytool_path.is_some()
    }

    pub fn javac_ready(&self) -> bool {
        self.javac_path.is_some()
    }

    pub fn version_label(&self) -> &str {
        self.version_text.as_deref().unwrap_or("未检测到")
    }
}

pub fn set_json_mode(enabled: bool) {
    JSON_MODE.store(enabled, Ordering::SeqCst);
}

pub fn is_json_mode() -> bool {
    JSON_MODE.load(Ordering::Relaxed)
}

pub fn run_command<S: AsRef<OsStr>>(cmd: S, args: &[&str], cwd: Option<&Path>) -> Result<String> {
    let cmd_ref = cmd.as_ref();
    log::debug!("执行命令: {:?} {}", cmd_ref, args.join(" "));

    let mut command = no_window_command(cmd_ref);
    command
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    if let Some(dir) = cwd {
        command.current_dir(dir);
    }

    let output = command
        .output()
        .with_context(|| format!("执行命令失败: {:?}", cmd_ref))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("命令执行失败: {:?}\n错误: {}", cmd_ref, stderr);
    }

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    Ok(stdout)
}

pub fn no_window_command<S: AsRef<OsStr>>(prog: S) -> Command {
    #[allow(unused_mut)]
    let mut cmd = Command::new(prog);
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
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
        .and_then(|p| p.parent().map(strip_unc_prefix))
        .unwrap_or_else(|| PathBuf::from("."))
}

pub fn strip_unc_prefix(path: &Path) -> PathBuf {
    dunce::simplified(path).to_path_buf()
}

pub fn dev_project_root() -> PathBuf {
    let current = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
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

fn install_java_hint() -> String {
    #[cfg(target_os = "macos")]
    {
        "建议安装：brew install --cask temurin17".to_string()
    }

    #[cfg(target_os = "windows")]
    {
        return "建议安装：scoop install temurin17-jdk".to_string();
    }

    #[cfg(target_os = "linux")]
    {
        return "请安装 JDK 17+，并确保 java / keytool / javac 可执行。".to_string();
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
        return "请安装 JDK 17+，并确保 java / keytool / javac 可执行。".to_string();
    }
}

fn missing_java_message() -> String {
    format!(
        "未检测到 Java {}+ 运行环境，签名、Alias 识别和加固流程无法继续。\n\
         请安装完整 JDK {}+，并确保 java / keytool / javac 在 PATH 中。\n{}",
        MIN_JAVA_MAJOR_VERSION,
        MIN_JAVA_MAJOR_VERSION,
        install_java_hint()
    )
}

fn find_java_binary(name: &str) -> Option<PathBuf> {
    which::which(name)
        .ok()
        .or_else(|| java_home_bin(name).filter(|path| path.exists()))
}

fn java_home_bin(name: &str) -> Option<PathBuf> {
    let java_home = std::env::var_os("JAVA_HOME")?;
    let mut path = PathBuf::from(java_home);
    path.push("bin");
    path.push(executable_name(name));
    Some(path)
}

fn executable_name(name: &str) -> String {
    #[cfg(target_os = "windows")]
    {
        format!("{name}.exe")
    }

    #[cfg(not(target_os = "windows"))]
    {
        name.to_string()
    }
}

fn read_java_version(java_path: &Path) -> Result<(Option<String>, Option<u32>)> {
    let output = no_window_command(java_path)
        .arg("-version")
        .output()
        .context("执行 java -version 失败")?;

    let text = if output.stderr.is_empty() {
        String::from_utf8_lossy(&output.stdout).to_string()
    } else {
        String::from_utf8_lossy(&output.stderr).to_string()
    };

    Ok(parse_java_version_text(&text))
}

fn parse_java_version_text(text: &str) -> (Option<String>, Option<u32>) {
    let version_text = extract_java_version_token(text);
    let major_version = version_text.as_deref().and_then(parse_java_major_version);
    (version_text, major_version)
}

fn extract_java_version_token(text: &str) -> Option<String> {
    for line in text.lines() {
        if let Some(start) = line.find('"') {
            let rest = &line[start + 1..];
            if let Some(end) = rest.find('"') {
                let token = rest[..end].trim();
                if !token.is_empty() {
                    return Some(token.to_string());
                }
            }
        }
    }

    text.split_whitespace()
        .find(|part| part.chars().next().is_some_and(|ch| ch.is_ascii_digit()))
        .map(|part| part.trim_matches('"').to_string())
}

fn parse_java_major_version(version: &str) -> Option<u32> {
    let first = version.split(['.', '_', '-']).next()?;
    if first == "1" {
        return version.split(['.', '_']).nth(1)?.parse().ok();
    }
    first.parse().ok()
}

pub fn find_java() -> Result<PathBuf> {
    Ok(ensure_java_runtime()?.java_path.unwrap())
}

pub fn find_keytool() -> Result<PathBuf> {
    Ok(ensure_keytool()?.keytool_path.unwrap())
}

pub fn find_javac() -> Result<PathBuf> {
    Ok(ensure_javac()?.javac_path.unwrap())
}

pub fn probe_java_environment() -> JavaEnvironmentInfo {
    let java_path = find_java_binary("java");
    let keytool_path = find_java_binary("keytool");
    let javac_path = find_java_binary("javac");
    let (version_text, major_version) = java_path
        .as_ref()
        .and_then(|path| read_java_version(path).ok())
        .unwrap_or((None, None));

    JavaEnvironmentInfo {
        java_path,
        keytool_path,
        javac_path,
        version_text,
        major_version,
    }
}

pub fn ensure_java_runtime() -> Result<JavaEnvironmentInfo> {
    let info = probe_java_environment();

    if info.java_path.is_none() {
        anyhow::bail!(missing_java_message());
    }

    if !info.java_ready() {
        anyhow::bail!(
            "检测到的 Java 版本过低：{}，需要 Java {}+。\n{}",
            info.version_label(),
            MIN_JAVA_MAJOR_VERSION,
            install_java_hint()
        );
    }

    Ok(info)
}

pub fn ensure_keytool() -> Result<JavaEnvironmentInfo> {
    let info = ensure_java_runtime()?;
    if !info.keytool_ready() {
        anyhow::bail!(
            "未检测到 keytool，请安装完整 JDK {}+，并确保 java / keytool 在 PATH 中。\n{}",
            MIN_JAVA_MAJOR_VERSION,
            install_java_hint()
        );
    }
    Ok(info)
}

pub fn ensure_javac() -> Result<JavaEnvironmentInfo> {
    let info = ensure_java_runtime()?;
    if !info.javac_ready() {
        anyhow::bail!(
            "未检测到 javac，请安装完整 JDK {}+，并确保 java / javac 在 PATH 中。\n{}",
            MIN_JAVA_MAJOR_VERSION,
            install_java_hint()
        );
    }
    Ok(info)
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

#[cfg(test)]
mod tests {
    use super::{extract_java_version_token, parse_java_major_version, parse_java_version_text};

    #[test]
    fn parse_modern_java_version() {
        let text = r#"openjdk version "17.0.12" 2024-07-16"#;
        let (version, major) = parse_java_version_text(text);
        assert_eq!(version.as_deref(), Some("17.0.12"));
        assert_eq!(major, Some(17));
    }

    #[test]
    fn parse_legacy_java_version() {
        let text = r#"java version "1.8.0_412""#;
        assert_eq!(
            extract_java_version_token(text).as_deref(),
            Some("1.8.0_412")
        );
        assert_eq!(parse_java_major_version("1.8.0_412"), Some(8));
    }

    #[test]
    fn parse_ea_java_version() {
        assert_eq!(parse_java_major_version("21-ea"), Some(21));
    }
}

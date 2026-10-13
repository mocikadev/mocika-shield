//! 报告的有限上下文和有界离线探测；不返回真实路径、完整属性或原始输出。
use serde::Serialize;
use shield_core::utils::{find_java, java_command};
use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

#[derive(Clone, Debug, Serialize, Default)]
pub(crate) struct ReportDetails {
    pub(crate) execution_step: &'static str,
    pub(crate) os_version: Option<String>,
    pub(crate) java_version: Option<String>,
    pub(crate) input_path: Option<PathFeatures>,
    pub(crate) output_path: Option<PathFeatures>,
    pub(crate) evidence_status: &'static str,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct PathFeatures {
    non_ascii: bool,
    spaces: bool,
    special_characters: bool,
    long_path: bool,
}
impl PathFeatures {
    pub(crate) fn of(path: &str) -> Self {
        Self {
            non_ascii: !path.is_ascii(),
            spaces: path.chars().any(char::is_whitespace),
            special_characters: path.chars().any(|c| "&%!'\"()[]{};`$".contains(c)),
            long_path: path.encode_utf16().count() > 240,
        }
    }
}

pub(crate) fn execution_step(step: &str) -> &'static str {
    match step {
        "CheckTools" | "PrepareSign" => "prepare",
        "Unpack" => "unpack",
        "ModifyManifest" => "modify_manifest",
        "ProcessDex" => "process_dex",
        "InjectRuntime" => "inject_runtime",
        "Repack" => "repack",
        "AlignApk" => "align",
        "SignApk" => "sign",
        "Cleanup" => "cleanup",
        _ => "unknown",
    }
}

/// 只接受完整数字版本，不把任意版本原文透传。Java 8 下划线更新号改为点号。
pub(crate) fn numeric_version(value: &str) -> Option<String> {
    let value = value.trim().replace('_', ".");
    if value.is_empty()
        || value.len() > 32
        || value
            .split('.')
            .any(|part| part.is_empty() || !part.bytes().all(|b| b.is_ascii_digit()))
    {
        return None;
    }
    Some(value)
}

fn vendor(value: &str) -> &'static str {
    let value = value.to_ascii_lowercase();
    if value.contains("amazon") {
        "amazon"
    } else if value.contains("adoptium") || value.contains("temurin") {
        "adoptium"
    } else if value.contains("azul") {
        "azul"
    } else if value.contains("microsoft") {
        "microsoft"
    } else if value.contains("oracle") {
        "oracle"
    } else if value.contains("openjdk") {
        "openjdk"
    } else {
        "unknown"
    }
}

// 每条流最多读取 16 KiB；超时杀死直接子进程，不等待未知后代持有的输出管道。
fn bounded_output(mut command: Command) -> Option<String> {
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .ok()?;
    let (send, recv) = std::sync::mpsc::channel();
    let stdout = child.stdout.take()?;
    let stderr = child.stderr.take()?;
    for stream in [Box::new(stdout) as Box<dyn Read + Send>, Box::new(stderr)] {
        let send = send.clone();
        std::thread::spawn(move || {
            let mut bytes = Vec::new();
            let _ = stream.take(16384).read_to_end(&mut bytes);
            let _ = send.send(bytes);
        });
    }
    drop(send);
    let deadline = Instant::now() + Duration::from_millis(1500);
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => break,
            Ok(Some(_)) => return None,
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
            _ if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
            _ => std::thread::sleep(Duration::from_millis(10)),
        }
    }
    let mut text = String::new();
    for _ in 0..2 {
        let bytes = recv
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .ok()?;
        text.push_str(std::str::from_utf8(&bytes).ok()?);
        text.push('\n');
    }
    Some(text)
}

pub(crate) fn enrich(report: &mut crate::failure_diagnostic::SafeReport, tool_path: Option<&Path>) {
    let Ok(java) = find_java() else {
        return;
    };
    let mut command = java_command(&java);
    command.args(["-XshowSettings:properties", "-version"]);
    if let Some(text) = bounded_output(command) {
        for line in text.lines() {
            let Some((key, value)) = line.trim().split_once(" = ") else {
                continue;
            };
            match key {
                "java.version" => {
                    report.details.java_version = numeric_version(value);
                    report.java_major = report
                        .details
                        .java_version
                        .as_deref()
                        .and_then(|v| {
                            let mut parts = v.split('.');
                            let first = parts.next()?;
                            if first == "1" {
                                parts.next()?.parse().ok()
                            } else {
                                first.parse().ok()
                            }
                        })
                        .filter(|v| (8..=99).contains(v));
                }
                "java.vendor" => report.java_vendor = Some(vendor(value)),
                "os.version" => report.details.os_version = numeric_version(value),
                _ => {}
            }
        }
    }
    // keytool 可能来自兜底路径中的另一个 JDK，不能用所选 JVM 版本冒充其版本。
    if report.tool_name == Some(shield_core::diagnostic::ToolName::Java) {
        report.tool_version = report.details.java_version.clone();
    } else if let Some(path) = tool_path {
        let mut command = java_command(java);
        command.arg("-jar").arg(path).arg("--version");
        report.tool_version =
            bounded_output(command).and_then(|text| text.lines().find_map(numeric_version));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 中文特殊字符路径只留下特征() {
        let value =
            serde_json::to_string(&PathFeatures::of("C:\\Users\\客户\\secret 应用&test.apk"))
                .unwrap();
        assert!(value.contains("\"non_ascii\":true"));
        assert!(value.contains("\"spaces\":true"));
        assert!(!value.contains("secret"));
        assert!(!value.contains("客户"));
    }

    #[test]
    fn 版本不允许路径和任意字符串() {
        assert_eq!(numeric_version("1.8.0_504"), Some("1.8.0.504".into()));
        for value in [
            "/Users/private/17",
            "17.0.2 secret",
            "1..2",
            "17-ea",
            "",
            "https://1.2",
        ] {
            assert_eq!(numeric_version(value), None);
        }
        assert_eq!(execution_step("Repack"), "repack");
        assert_eq!(execution_step("ModifyManifest"), "modify_manifest");
        assert_eq!(vendor("PRIVATE_VENDOR"), "unknown");
    }

    #[test]
    #[ignore = "需要真实 JDK 与仓库内 apktool，手动运行"]
    fn 真实工具失败报告取得版本并舍弃中文路径() {
        use crate::failure_diagnostic::{FailureContext, SafeReport};
        use shield_core::diagnostic::{Diagnostic, ToolName};
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("私密客户 不存在.apk");
        let jar = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../tools/apktool_3.0.1.jar");
        let output = java_command(find_java().unwrap())
            .arg("-jar")
            .arg(&jar)
            .arg("d")
            .arg(&input)
            .output()
            .unwrap();
        assert!(!output.status.success());
        let diagnostic = Diagnostic::from_tool_streams(
            ToolName::Apktool,
            output.status.code(),
            &output.stdout,
            &output.stderr,
        );
        let mut report = SafeReport::new(
            FailureContext::protect("Unpack", false, false),
            diagnostic,
            1,
        );
        report.details.execution_step = "unpack";
        report.details.input_path = Some(PathFeatures::of(input.to_str().unwrap()));
        enrich(&mut report, Some(&jar));
        assert!(report.java_major.is_some());
        assert!(report.details.java_version.is_some());
        assert_eq!(report.tool_version.as_deref(), Some("3.0.1"));
        assert!(report
            .evidence
            .iter()
            .any(|item| item.kind == "file_not_found"));
        let json = serde_json::to_string(&report).unwrap();
        assert!(!json.contains("私密客户"));
        assert!(!json.contains(dir.path().to_str().unwrap()));
        println!("安全报告样本：{json}");
    }

    #[cfg(unix)]
    #[test]
    fn 探测超时有界且失败不返回输出() {
        let mut command = shield_core::utils::no_window_command("sh");
        command.args(["-c", "exec sleep 5"]);
        let started = Instant::now();
        assert!(bounded_output(command).is_none());
        assert!(started.elapsed() < Duration::from_secs(3));
    }
}

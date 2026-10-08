//! 关联所选 Java 与同一安装中的工具，避免分别探测造成漏报。

use std::path::{Path, PathBuf};

use crate::utils::{no_window_command, strip_unc_prefix};

pub(crate) fn find_java() -> Option<PathBuf> {
    which::which("java")
        .ok()
        .map(|path| strip_unc_prefix(&path))
        .or_else(|| environment_home().and_then(|home| home_tool(&home, "java")))
}

pub(crate) fn find_keytool(java: Option<&Path>) -> Option<PathBuf> {
    resolve_keytool(
        java,
        environment_home().as_deref(),
        || {
            which::which("keytool")
                .ok()
                .map(|path| strip_unc_prefix(&path))
        },
        read_runtime_home,
    )
}

fn environment_home() -> Option<PathBuf> {
    std::env::var_os("JAVA_HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

fn valid_tool(path: &Path) -> Option<PathBuf> {
    // 使用 which 统一校验普通文件及执行权限，不把同名目录视为工具。
    // 先跟随链接检查普通文件，避免 Windows 将失效的 .exe 链接视为有效候选。
    if !path.is_file() {
        return None;
    }
    which::which(strip_unc_prefix(path))
        .ok()
        .map(|path| strip_unc_prefix(&path))
}

fn home_tool(home: &Path, name: &str) -> Option<PathBuf> {
    valid_tool(&home.join("bin").join(executable_name(name)))
}

fn resolve_keytool(
    java: Option<&Path>,
    java_home: Option<&Path>,
    path_keytool: impl FnOnce() -> Option<PathBuf>,
    runtime_home: impl FnOnce(&Path) -> Option<PathBuf>,
) -> Option<PathBuf> {
    if let Some(java) = java {
        let real_java = std::fs::canonicalize(java).unwrap_or_else(|_| java.to_path_buf());
        if let Some(tool) = real_java
            .parent()
            .and_then(|bin| valid_tool(&bin.join(executable_name("keytool"))))
        {
            return Some(tool);
        }
        if let Some(home) = runtime_home(java) {
            if let Some(tool) = home_tool(&home, "keytool") {
                return Some(tool);
            }
            // Java 8 的 java.home 可以指向 JDK 内的 jre，而工具位于 JDK/bin。
            if home
                .file_name()
                .is_some_and(|name| name.eq_ignore_ascii_case("jre"))
            {
                if let Some(tool) = home.parent().and_then(|jdk| home_tool(jdk, "keytool")) {
                    return Some(tool);
                }
            }
        }
    }
    java_home
        .and_then(|home| home_tool(home, "keytool"))
        .or_else(path_keytool)
}

fn read_runtime_home(java: &Path) -> Option<PathBuf> {
    let output = no_window_command(strip_unc_prefix(java))
        .args([
            "-Dfile.encoding=UTF-8",
            "-Dsun.stdout.encoding=UTF-8",
            "-Dsun.stderr.encoding=UTF-8",
            "-Dstdout.encoding=UTF-8",
            "-Dstderr.encoding=UTF-8",
            "-XshowSettings:properties",
            "-version",
        ])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    parse_runtime_home(&String::from_utf8_lossy(&output.stderr))
        .or_else(|| parse_runtime_home(&String::from_utf8_lossy(&output.stdout)))
}

fn parse_runtime_home(text: &str) -> Option<PathBuf> {
    text.lines().find_map(|line| {
        let (key, value) = line.split_once('=')?;
        let value = value.trim();
        (key.trim() == "java.home" && !value.is_empty()).then(|| PathBuf::from(value))
    })
}

fn executable_name(name: &str) -> String {
    if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn executable(root: &Path, relative: &str) -> PathBuf {
        let path = root.join(relative).join(executable_name("keytool"));
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, b"fixture").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        path
    }

    #[test]
    fn selected_java_sibling_wins_over_other_installations() {
        let dir = tempfile::tempdir().unwrap();
        let sibling = executable(dir.path(), "中文 JDK/bin");
        let other = executable(dir.path(), "other/bin");
        let java = sibling.parent().unwrap().join(executable_name("java"));
        assert_eq!(
            resolve_keytool(
                Some(&java),
                Some(&dir.path().join("other")),
                || Some(other),
                |_| panic!("同目录已命中，不应启动 Java")
            ),
            Some(sibling)
        );
    }

    #[test]
    fn launcher_uses_runtime_home_before_environment_fallback() {
        let dir = tempfile::tempdir().unwrap();
        let keytool = executable(dir.path(), "真实 JDK/bin");
        executable(dir.path(), "home/bin");
        let path_tool = executable(dir.path(), "path/bin");
        assert_eq!(
            resolve_keytool(
                Some(&dir.path().join("launcher/java")),
                Some(&dir.path().join("home")),
                || Some(path_tool),
                |_| Some(dir.path().join("真实 JDK"))
            ),
            Some(keytool)
        );
    }

    #[test]
    fn java8_jre_finds_parent_jdk() {
        let dir = tempfile::tempdir().unwrap();
        let keytool = executable(dir.path(), "jdk8/bin");
        assert_eq!(
            resolve_keytool(
                Some(&dir.path().join("launcher/java")),
                None,
                || None,
                |_| Some(dir.path().join("jdk8/jre"))
            ),
            Some(keytool)
        );
    }

    #[test]
    fn java_home_precedes_unrelated_path_keytool() {
        let dir = tempfile::tempdir().unwrap();
        let home_tool = executable(dir.path(), "home/bin");
        let path_tool = executable(dir.path(), "path/bin");
        assert_eq!(
            resolve_keytool(
                None,
                Some(&dir.path().join("home")),
                || Some(path_tool),
                |_| None
            ),
            Some(home_tool)
        );
    }

    #[test]
    fn rejects_directory_as_tool() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("bin").join(executable_name("keytool"))).unwrap();
        assert_eq!(
            resolve_keytool(None, Some(dir.path()), || None, |_| None),
            None
        );
    }

    #[test]
    fn missing_runtime_and_home_fall_back_to_path() {
        let dir = tempfile::tempdir().unwrap();
        let keytool = executable(dir.path(), "path/bin");
        assert_eq!(
            resolve_keytool(
                Some(&dir.path().join("missing/java")),
                Some(&dir.path().join("missing")),
                || Some(keytool.clone()),
                |_| None
            ),
            Some(keytool)
        );
        assert_eq!(
            resolve_keytool(
                None,
                None,
                || None,
                |_| panic!("没有 Java 不应查询运行目录")
            ),
            None
        );
    }

    #[test]
    fn parses_only_nonempty_java_home_preserving_spaces_and_equals() {
        assert_eq!(
            parse_runtime_home(
                "    user.home = /other\r\n    java.home = C:\\中文 JDK=11\\home\r\n"
            ),
            Some(PathBuf::from("C:\\中文 JDK=11\\home"))
        );
        assert_eq!(parse_runtime_home("java.home = \nuser.home = /other"), None);
    }

    #[cfg(unix)]
    #[test]
    fn rejects_tool_without_execute_permission() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let tool = executable(dir.path(), "bin");
        std::fs::set_permissions(tool, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert_eq!(
            resolve_keytool(None, Some(dir.path()), || None, |_| None),
            None
        );
    }

    #[cfg(unix)]
    #[test]
    fn symlink_java_uses_real_installation() {
        let dir = tempfile::tempdir().unwrap();
        let keytool = executable(dir.path(), "jdk/bin");
        let real_java = keytool.parent().unwrap().join("java");
        std::fs::write(&real_java, b"fixture").unwrap();
        let link = dir.path().join("java");
        std::os::unix::fs::symlink(real_java, &link).unwrap();
        assert_eq!(
            resolve_keytool(Some(&link), None, || None, |_| panic!("真实目录已命中")),
            Some(strip_unc_prefix(&std::fs::canonicalize(keytool).unwrap()))
        );
    }

    #[cfg(unix)]
    #[test]
    fn dangling_tool_link_does_not_block_home_fallback() {
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join("jdk/bin");
        std::fs::create_dir_all(&bin).unwrap();
        std::os::unix::fs::symlink(dir.path().join("missing-keytool"), bin.join("keytool"))
            .unwrap();
        let home_tool = executable(dir.path(), "home/bin");
        assert_eq!(
            resolve_keytool(
                Some(&bin.join("java")),
                Some(&dir.path().join("home")),
                || None,
                |_| None
            ),
            Some(home_tool)
        );
    }
}

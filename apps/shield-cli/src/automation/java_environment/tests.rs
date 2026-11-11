use super::*;
use std::{
    fs,
    io::Read,
    process::Stdio,
    time::{Duration, Instant},
};

fn command() -> Command {
    shield_core::utils::no_window_command("未启动的测试命令")
}

#[test]
fn 只为本次子进程引用完整选项且父环境不变() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("中文 空格='测试");
    fs::create_dir(&path).unwrap();
    let original: Vec<_> = ["JAVA_TOOL_OPTIONS", "TMPDIR", "TMP", "TEMP"]
        .map(std::env::var_os)
        .into();
    let mut cmd = command();
    assert!(apply(&mut cmd, &path).is_ok());
    let configured = std::path::PathBuf::from(
        cmd.get_envs()
            .find(|(k, _)| *k == "TMPDIR")
            .unwrap()
            .1
            .unwrap(),
    );
    assert_eq!(
        crate::automation::task_files::resolved(&configured).unwrap(),
        crate::automation::task_files::resolved(&path).unwrap()
    );
    #[cfg(windows)]
    assert!(configured.to_str().unwrap().is_ascii());
    let path = configured;
    for key in ["TMPDIR", "TMP", "TEMP"] {
        assert_eq!(
            cmd.get_envs().find(|(k, _)| *k == key).unwrap().1,
            Some(path.as_os_str())
        );
    }
    assert_eq!(
        cmd.get_envs()
            .find(|(k, _)| *k == "JAVA_TOOL_OPTIONS")
            .unwrap()
            .1
            .unwrap(),
        format!(
            "\"-Duser.home={}\" \"-Djava.io.tmpdir={}\"",
            path.display(),
            path.display()
        )
        .as_str()
    );
    assert_eq!(
        original,
        ["JAVA_TOOL_OPTIONS", "TMPDIR", "TMP", "TEMP"].map(std::env::var_os)
    );
}

#[test]
fn 拒绝相对路径引号控制符及会被旧版忽略的超长选项() {
    let root = tempfile::tempdir().unwrap();
    for path in [
        Path::new("relative").to_owned(),
        root.path().join("bad\"value"),
        root.path().join("bad\nvalue"),
        root.path().join("bad\0value"),
        root.path().join("x".repeat(600)),
    ] {
        let mut cmd = command();
        assert!(apply(&mut cmd, &path).is_err());
        assert_eq!(cmd.get_envs().count(), 0);
    }
}

#[cfg(windows)]
#[test]
fn 规范化不能安全去掉扩展前缀时拒绝传给java() {
    let path = Path::new(r"\\?\C:\CON");
    let mut cmd = command();
    assert!(apply(&mut cmd, path).is_err());
    assert_eq!(cmd.get_envs().count(), 0);
}

// 探针显式运行；不以 ignored 计数代替真实 JDK 验证。不读业务文件或真实资料库。
#[test]
#[ignore = "需要显式提供 SHIELD_TEST_JAVA_HOME 的真实 JDK"]
fn 真实jdk中文空格目录属性往返一致() {
    let home = std::env::var_os("SHIELD_TEST_JAVA_HOME").expect("需要指定本次探针的 JDK");
    let bin = Path::new(&home).join("bin");
    let java = bin.join(if cfg!(windows) { "java.exe" } else { "java" });
    let javac = bin.join(if cfg!(windows) { "javac.exe" } else { "javac" });
    assert!(java.is_file() && javac.is_file(), "需要完整 JDK");
    let root = tempfile::tempdir().unwrap();
    let classes = root.path().join("classes");
    fs::create_dir(&classes).unwrap();
    let source = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/agent-runtime/RuntimeProperties.java");
    let mut compile = shield_core::utils::no_window_command(javac);
    compile.env_remove("JAVA_TOOL_OPTIONS");
    compile
        .args(["-encoding", "UTF-8", "-source", "8", "-target", "8", "-d"])
        .arg(&classes)
        .arg(source);
    assert!(run(compile, root.path(), "编译").is_empty());
    let class = fs::read(classes.join("RuntimeProperties.class")).unwrap();
    assert_eq!(&class[..8], &[0xca, 0xfe, 0xba, 0xbe, 0, 0, 0, 52]);
    for name in ["plain space=value", "中文 空格='目录"] {
        let work = root.path().join(name);
        fs::create_dir(&work).unwrap();
        // 使用系统规范化路径，Windows 由生产辅助函数处理系统返回的扩展 UNC。
        let work = fs::canonicalize(&work).unwrap();
        // 与签名／加固相同的核心命令构造，包含实际编码参数及无窗口设置。
        let mut cmd = shield_core::utils::java_command(&java);
        cmd.arg("-cp").arg(&classes).arg("RuntimeProperties");
        assert!(apply(&mut cmd, &work).is_ok());
        let expected = std::path::PathBuf::from(
            cmd.get_envs()
                .find(|(k, _)| *k == "TMPDIR")
                .unwrap()
                .1
                .unwrap(),
        );
        assert_eq!(
            crate::automation::task_files::resolved(&expected).unwrap(),
            crate::automation::task_files::resolved(&work).unwrap()
        );
        let output = run(cmd, root.path(), "运行");
        let mut input = output.as_slice();
        for _ in 0..2 {
            assert_eq!(read_property(&mut input), expected.to_str().unwrap());
        }
        assert!(work.join("home.marker").is_file());
        assert!(work.join("temp.marker").is_file());
        let version = read_property(&mut input);
        assert!(!version.is_empty());
        assert!(input.is_empty());
        println!(
            "Java 属性探针通过：版本={version}，目录类别={}",
            if name.starts_with("plain") {
                "空格及等号"
            } else {
                "中文及引号"
            }
        );
    }
}

#[cfg(windows)]
#[test]
fn 中文父目录下活动工作根的锁不妨碍java身份核对() {
    use crate::automation::task_files::{open_directory, same_directory, WorkDirectory};
    let root = tempfile::tempdir().unwrap();
    let parent = root.path().join("中文 父目录");
    fs::create_dir(&parent).unwrap();
    let directory = open_directory(&parent).unwrap();
    let work = WorkDirectory::new(&parent, &directory).unwrap();
    let mut cmd = command();
    assert!(apply(&mut cmd, work.path()).is_ok());
    let alias = std::path::PathBuf::from(
        cmd.get_envs()
            .find(|(k, _)| *k == "TMPDIR")
            .unwrap()
            .1
            .unwrap(),
    );
    assert!(same_directory(work.path(), &alias).unwrap());
    assert!(alias.to_str().unwrap().is_ascii());
    work.close().unwrap();
}

fn read_property(input: &mut &[u8]) -> String {
    let mut bytes = [0; 4];
    input.read_exact(&mut bytes).unwrap();
    let len = u32::from_be_bytes(bytes) as usize;
    assert!(len <= 8192 && len <= input.len(), "探针帧长度无效");
    let (value, rest) = input.split_at(len);
    *input = rest;
    String::from_utf8(value.to_vec()).unwrap()
}

fn run(mut cmd: Command, root: &Path, stage: &str) -> Vec<u8> {
    // 仅运行自有有限探针；用临时文件承接输出，避免未读管道阻塞，并有界等待。
    let stdout = tempfile::tempfile_in(root).unwrap();
    let stderr = tempfile::tempfile_in(root).unwrap();
    cmd.stdin(Stdio::null())
        .stdout(stdout.try_clone().unwrap())
        .stderr(stderr.try_clone().unwrap());
    for key in ["_JAVA_OPTIONS", "JDK_JAVA_OPTIONS"] {
        cmd.env_remove(key);
    }
    let mut child = cmd.spawn().expect("探针启动失败");
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            if !status.success() {
                // 只回读自有编译器／探针的有限诊断，不与生产错误或业务日志共用。
                use std::io::{Seek, SeekFrom};
                let mut stderr = stderr;
                stderr.seek(SeekFrom::Start(0)).unwrap();
                let mut detail = Vec::new();
                stderr.take(4096).read_to_end(&mut detail).unwrap();
                panic!(
                    "自有 Java 探针{stage}失败：{status}，{}",
                    String::from_utf8_lossy(&detail)
                );
            }
            break;
        }
        if Instant::now() >= deadline {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("自有 Java 探针超时，已停止并回收");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    use std::io::{Seek, SeekFrom};
    let mut stdout = stdout;
    stdout.seek(SeekFrom::Start(0)).unwrap();
    assert!(stdout.metadata().unwrap().len() <= 32 * 1024);
    let mut bytes = Vec::new();
    stdout.read_to_end(&mut bytes).unwrap();
    bytes
}

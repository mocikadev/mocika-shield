//! 仅测试：由真实 apktool 通过 --aapt 启动，执行原 JAR 内程序后保留返回等待窗口。
use std::{
    fs,
    io::{Read, Write},
    path::Path,
    process::{Command, Stdio},
    time::Duration,
};
#[cfg(windows)]
#[path = "NativeOutput.rs"]
mod output;

fn command(program: impl AsRef<std::ffi::OsStr>) -> Command {
    let command = Command::new(program);
    #[cfg(windows)]
    let command = {
        use std::os::windows::process::CommandExt;
        let mut command = command;
        command.creation_flags(0x08000000);
        command
    };
    command
}

fn mark(root: &Path, name: &str, bytes: &[u8]) {
    let pending = root.join(format!(".{name}.pending"));
    fs::write(&pending, bytes).unwrap();
    fs::rename(pending, root.join(name)).unwrap();
}

fn writer(root: &Path) {
    assert!(std::env::var_os("MOCIKA_SHIELD_TOOL_KS_PASS").is_none());
    assert!(std::env::var_os("MOCIKA_SHIELD_TOOL_KEY_PASS").is_none());
    let mut byte = [0];
    assert_eq!(std::io::stdin().read(&mut byte).unwrap(), 0);
    mark(
        root,
        "native-writer-pid",
        std::process::id().to_string().as_bytes(),
    );
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(root.join("heartbeat"))
        .unwrap();
    for _ in 0..12000 {
        file.write_all(b".").unwrap();
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn capture(input: impl Read) -> Vec<u8> {
    let mut bytes = Vec::new();
    input.take(65537).read_to_end(&mut bytes).unwrap();
    assert!(bytes.len() <= 65536, "实际 aapt2 输出不应超出夹具预算");
    bytes
}

fn main() {
    let root = std::path::PathBuf::from(std::env::var_os("MOCIKA_TEST_GROUP_ROOT").unwrap());
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.first().is_some_and(|arg| arg == "--writer") {
        writer(&root);
        return;
    }
    let decode = args.first().is_some_and(|arg| arg == "--decode-wait");
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    if !decode {
        #[cfg(windows)]
        assert!(
            args.iter().all(|arg| arg.is_ascii()),
            "本次 Native 回归参数必须通过同目录 ASCII 名称进入实际工具"
        );
        assert!(
            args.iter().any(|arg| arg == "compile"
                || arg == "link"
                || arg == "version"
                || arg == "--version"),
            "必须命中实际 aapt2 命令"
        );
        let mut real = command(std::env::var_os("MOCIKA_TEST_REAL_AAPT").unwrap())
            .args(&args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let out = real.stdout.take().unwrap();
        let err = real.stderr.take().unwrap();
        let result = std::thread::scope(|scope| {
            let out = scope.spawn(|| capture(out));
            let err = scope.spawn(|| capture(err));
            let status = real.wait().unwrap();
            (status, out.join().unwrap(), err.join().unwrap())
        });
        if !result.0.success()
            || args
                .iter()
                .any(|arg| arg == "version" || arg == "--version")
        {
            std::io::stdout().write_all(&result.1).unwrap();
            std::io::stderr().write_all(&result.2).unwrap();
            std::process::exit(result.0.code().unwrap_or(1));
        }
        // 并行编译时仅选中一处；其他实际调用正常返回，不共享门闩或伪造阶段。
        match fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(root.join("native-selected"))
        {
            Ok(_) => (),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                std::io::stdout().write_all(&result.1).unwrap();
                std::io::stderr().write_all(&result.2).unwrap();
                return;
            }
            Err(error) => panic!("原生调用选择失败：{error}"),
        }
        mark(&root, "aapt-completed", b"0");
        stdout = result.1;
        stderr = result.2;
    } else {
        let mut byte = [0];
        assert_eq!(std::io::stdin().read(&mut byte).unwrap(), 0);
    }
    #[cfg(windows)]
    output::isolate(&root);
    let mut writer = command(std::env::current_exe().unwrap())
        .arg("--writer")
        .env_remove("MOCIKA_SHIELD_TOOL_KS_PASS")
        .env_remove("MOCIKA_SHIELD_TOOL_KEY_PASS")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    mark(
        &root,
        "native-proxy-pid",
        std::process::id().to_string().as_bytes(),
    );
    mark(
        &root,
        "apktool-window-ready",
        if decode { b"decode" } else { b"native" },
    );
    let mode = std::env::var("MOCIKA_TEST_APKTOOL_MODE").unwrap();
    if mode == "flood" || mode == "flood-hold" {
        let bytes = vec![b'x'; 1024 * 1024];
        std::thread::scope(|scope| {
            scope.spawn(|| {
                std::io::stdout().write_all(&bytes).unwrap();
                std::io::stdout().flush().unwrap();
            });
            scope.spawn(|| {
                std::io::stderr().write_all(&bytes).unwrap();
                std::io::stderr().flush().unwrap();
            });
        });
        mark(&root, "apktool-flood-written", b"yes");
    }
    if mode == "exit" || mode == "flood" {
        std::process::exit(41);
    }
    assert!(mode == "hold" || mode == "flood-hold");
    let _ = writer.wait();
    std::io::stdout().write_all(&stdout).unwrap();
    std::io::stderr().write_all(&stderr).unwrap();
}

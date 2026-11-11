//! 测试侧代理：严格识别签名／验签帧，执行正式助手，再制造返回等待或固定故障。
use std::{
    fs,
    io::{Read, Write},
    path::Path,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

#[cfg(windows)]
#[path = "NativeOutput.rs"]
mod output;

fn command(program: impl AsRef<std::ffi::OsStr>) -> Command {
    let mut result = Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        result.creation_flags(0x08000000);
    }
    result
}

fn mark(root: &Path, name: &str) {
    let pending = root.join(format!(".{name}.pending"));
    fs::write(&pending, b"yes").unwrap();
    fs::rename(pending, root.join(name)).unwrap();
}

fn frame(bytes: &[u8]) -> Vec<String> {
    assert!(bytes.len() >= 16 && bytes.len() <= 65536);
    assert_eq!(&bytes[..4], b"MSJA");
    assert_eq!(u32::from_be_bytes(bytes[4..8].try_into().unwrap()), 4);
    let count = u32::from_be_bytes(bytes[8..12].try_into().unwrap());
    assert!((3..=64).contains(&count));
    let mut offset = 12;
    let mut values = Vec::new();
    for _ in 0..count {
        let size = u32::from_be_bytes(bytes[offset..offset + 4].try_into().unwrap()) as usize;
        offset += 4;
        values.push(
            std::str::from_utf8(&bytes[offset..offset + size])
                .unwrap()
                .to_owned(),
        );
        offset += size;
    }
    assert_eq!(offset, bytes.len());
    assert_eq!(values[0], "apksigner");
    assert!(matches!(values[2].as_str(), "sign" | "verify"));
    for name in ["MOCIKA_SHIELD_TOOL_KS_PASS", "MOCIKA_SHIELD_TOOL_KEY_PASS"] {
        if let Ok(secret) = std::env::var(name) {
            assert!(
                !secret.is_empty()
                    && !bytes
                        .windows(secret.len())
                        .any(|part| part == secret.as_bytes())
            );
        }
    }
    values
}

fn writer(root: &Path) {
    assert!(std::env::var_os("MOCIKA_SHIELD_TOOL_KS_PASS").is_none());
    assert!(std::env::var_os("MOCIKA_SHIELD_TOOL_KEY_PASS").is_none());
    let mut byte = [0; 1];
    assert_eq!(std::io::stdin().read(&mut byte).unwrap(), 0);
    fs::write(root.join("child-pid"), std::process::id().to_string()).unwrap();
    mark(root, "signing-stdin-empty");
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

fn capture(mut input: impl Read) -> Vec<u8> {
    let mut bytes = Vec::new();
    let mut chunk = [0; 8192];
    loop {
        let count = input.read(&mut chunk).unwrap();
        if count == 0 {
            break;
        }
        let keep = count.min(65536 - bytes.len());
        bytes.extend_from_slice(&chunk[..keep]);
        assert_eq!(keep, count, "原始工具不应超过测试代理预算");
    }
    bytes
}

fn flood(root: &Path) {
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
    mark(root, "flood-written");
}

fn main() {
    let root = std::path::PathBuf::from(std::env::var_os("MOCIKA_TEST_GROUP_ROOT").unwrap());
    if std::env::args().nth(1).as_deref() == Some("--writer") {
        writer(&root);
        return;
    }
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    assert!(args.iter().any(|value| value == "java-adapter.jar"));
    let mut bytes = Vec::new();
    std::io::stdin()
        .take(65537)
        .read_to_end(&mut bytes)
        .unwrap();
    let values = frame(&bytes);
    let operation = &values[2];
    let phase = std::env::var("MOCIKA_TEST_SIGN_PHASE").unwrap();
    let mode = std::env::var("MOCIKA_TEST_SIGN_MODE").unwrap();
    let selected = phase == format!("{operation}-output");
    mark(&root, &format!("{operation}-request-entered"));
    if selected && mode == "permission" {
        mark(&root, "permission-ready");
        let until = Instant::now() + Duration::from_secs(60);
        while !root.join("permission-release").exists() {
            assert!(Instant::now() < until);
            std::thread::sleep(Duration::from_millis(5));
        }
    }
    let mut real = command(std::env::var_os("MOCIKA_TEST_REAL_JAVA").unwrap());
    real.args(args)
        .stdin(fs::File::open("request.bin").unwrap())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = real.spawn().unwrap();
    fs::write(
        root.join(format!("{operation}-java-pid")),
        child.id().to_string(),
    )
    .unwrap();
    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();
    let (status, stdout, stderr) = std::thread::scope(|scope| {
        let stdout = scope.spawn(|| capture(stdout));
        let stderr = scope.spawn(|| capture(stderr));
        let status = child.wait().unwrap();
        (status, stdout.join().unwrap(), stderr.join().unwrap())
    });
    fs::write(
        root.join(format!("{operation}-jvm-exit")),
        status.code().unwrap_or(-1).to_string(),
    )
    .unwrap();
    if !selected {
        assert!(status.success(), "非目标真实阶段失败");
        std::io::stdout().write_all(&stdout).unwrap();
        std::io::stderr().write_all(&stderr).unwrap();
        return;
    }
    assert_eq!(
        status.success(),
        mode != "permission",
        "原工具状态必须符合真实阶段预期"
    );
    fs::write(
        root.join(format!("{operation}-status")),
        status.code().unwrap_or(-1).to_string(),
    )
    .unwrap();
    #[cfg(windows)]
    output::isolate(&root);
    let mut writer = command(std::env::current_exe().unwrap())
        .arg("--writer")
        .env_remove("MOCIKA_SHIELD_TOOL_KS_PASS")
        .env_remove("MOCIKA_SHIELD_TOOL_KEY_PASS")
        .stdin(Stdio::inherit())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    mark(&root, &format!("{operation}-output-ready"));
    match mode.as_str() {
        "hold" => {
            let _ = writer.wait();
        }
        "flood-hold" => {
            flood(&root);
            let _ = writer.wait();
        }
        "exit" => std::process::exit(41),
        "flood" => {
            std::io::stdout().write_all(&stdout).unwrap();
            flood(&root);
            std::process::exit(if operation == "sign" { 41 } else { 0 });
        }
        "invalid-utf8" => {
            assert_eq!(operation, "verify");
            std::io::stdout().write_all(&stdout).unwrap();
            std::io::stdout().write_all(&[0xff]).unwrap();
            std::io::stdout().flush().unwrap();
        }
        "permission" => {
            std::io::stdout().write_all(&stdout).unwrap();
            std::io::stderr().write_all(&stderr).unwrap();
            std::process::exit(status.code().unwrap_or(42));
        }
        _ => panic!("未知签名代理模式"),
    }
}

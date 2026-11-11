//! 仅用于原生测试：识别真实新建阶段并委托原工具，再注入等待或失败，不进入正式资源。
use std::{
    fs,
    io::{Read, Write},
    path::Path,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

fn command(program: impl AsRef<std::ffi::OsStr>) -> Command {
    let mut command = Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    command
}

#[cfg(windows)]
#[path = "NativeOutput.rs"]
mod output;

fn mark(root: &Path, name: &str) {
    let pending = root.join(format!(".{name}.pending"));
    fs::write(&pending, b"yes").unwrap();
    fs::rename(pending, root.join(name)).unwrap();
}

fn writer(root: &Path) {
    let mut byte = [0u8; 1];
    assert_eq!(std::io::stdin().read(&mut byte).unwrap(), 0);
    mark(root, "certificate-stdin-empty");
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

fn operation(bytes: &[u8]) -> String {
    assert!(bytes.len() <= 65536 && bytes.len() >= 16);
    assert_eq!(&bytes[..4], b"MSJA");
    assert_eq!(u32::from_be_bytes(bytes[4..8].try_into().unwrap()), 4);
    let count = u32::from_be_bytes(bytes[8..12].try_into().unwrap());
    assert!(matches!(count, 7 | 8));
    let mut offset = 12;
    let mut result = String::new();
    for index in 0..count {
        let size = u32::from_be_bytes(bytes[offset..offset + 4].try_into().unwrap()) as usize;
        offset += 4;
        let value = std::str::from_utf8(&bytes[offset..offset + size]).unwrap();
        if index == 0 {
            result = value.to_owned();
        }
        offset += size;
    }
    assert_eq!(offset, bytes.len());
    result
}

fn main() {
    let root = std::path::PathBuf::from(std::env::var_os("MOCIKA_TEST_GROUP_ROOT").unwrap());
    if std::env::args().nth(1).as_deref() == Some("--writer") {
        writer(&root);
        return;
    }
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    let generate = args.iter().any(|arg| arg == "-genkeypair");
    let phase = if generate {
        "generate"
    } else {
        assert!(args.iter().any(|arg| arg == "java-adapter.jar"));
        let mut bytes = Vec::new();
        std::io::stdin()
            .take(65537)
            .read_to_end(&mut bytes)
            .unwrap();
        match operation(&bytes).as_str() {
            "keystore_create_prepare" => "prepare",
            "keystore_create_finish" => "finish",
            _ => panic!("未命中新建阶段，不能把其他操作算作新建监督通过"),
        }
    };
    let selected = std::env::var("MOCIKA_TEST_CERT_PHASE").unwrap() == phase;
    let mode = std::env::var("MOCIKA_TEST_CERT_MODE").unwrap();
    mark(&root, &format!("{phase}-entered"));
    if selected && mode == "permission" {
        mark(&root, "permission-ready");
        let end = Instant::now() + Duration::from_secs(60);
        while !root.join("permission-release").exists() {
            assert!(Instant::now() < end, "权限夹具未释放");
            std::thread::sleep(Duration::from_millis(5));
        }
    }
    let real = std::env::var_os(if generate {
        "MOCIKA_TEST_REAL_KEYTOOL"
    } else {
        "MOCIKA_TEST_REAL_JAVA"
    })
    .unwrap();
    let mut real = command(real);
    real.args(args).stdout(Stdio::piped()).stderr(Stdio::null());
    if generate {
        real.stdin(Stdio::null());
    } else {
        // 重开同一有限只读请求，而不是新建输入管道或传递控制端租约。
        real.stdin(fs::File::open("request.bin").unwrap());
    }
    let mut child = real.spawn().unwrap();
    let mut bytes = Vec::new();
    child
        .stdout
        .take()
        .unwrap()
        .take(65537)
        .read_to_end(&mut bytes)
        .unwrap();
    if bytes.len() > 65536 {
        let _ = child.kill();
        let _ = child.wait();
        panic!("真实工具输出超出夹具预算");
    }
    let status = child.wait().unwrap();
    if !selected {
        assert!(status.success(), "前置真实阶段失败，不能进入目标故障窗口");
        std::io::stdout().write_all(&bytes).unwrap();
        return;
    }
    if mode != "permission" {
        assert!(status.success(), "真实工具必须先执行成功");
    } else {
        assert!(!status.success(), "权限拒绝未实际生效");
    }
    fs::write(
        root.join(format!("{phase}-status")),
        status.code().unwrap_or(-1).to_string(),
    )
    .unwrap();
    mark(&root, &format!("{phase}-completed"));
    // 故障回执必须能达到 EOF；等待窗口由本代理持有，不让写心跳的后代意外延长输出。
    #[cfg(windows)]
    output::isolate(&root);
    let mut writer = command(std::env::current_exe().unwrap())
        .arg("--writer")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    fs::write(root.join("child-pid"), writer.id().to_string()).unwrap();
    let end = Instant::now() + Duration::from_secs(5);
    while !root
        .join("heartbeat")
        .metadata()
        .is_ok_and(|meta| meta.len() > 0)
    {
        assert!(Instant::now() < end);
        std::thread::sleep(Duration::from_millis(5));
    }
    match mode.as_str() {
        "hold" => {
            let _ = writer.wait();
        }
        "exit" => std::process::exit(41),
        "malformed" => {
            std::io::stdout()
                .write_all(b"invalid-certificate-result")
                .unwrap();
        }
        "flood" => {
            std::io::stdout().write_all(&vec![b'x'; 65537]).unwrap();
            let _ = writer.wait();
        }
        "permission" => {
            std::io::stdout().write_all(&bytes).unwrap();
            std::process::exit(status.code().unwrap_or(42));
        }
        _ => panic!("未知新建测试模式"),
    }
}

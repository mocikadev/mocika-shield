//! 原生故障夹具：真实工具保持委托，仅自有签名步骤挂起并创建有期限的后代。
use std::{
    env,
    ffi::OsStr,
    fs,
    io::{Read, Write},
    path::Path,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

fn no_window_command(program: impl AsRef<OsStr>) -> Command {
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
fn heartbeat(root: &Path, name: &str) {
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(root.join(name))
        .unwrap();
    let until = Instant::now() + Duration::from_secs(90);
    while Instant::now() < until && !root.join("release").exists() {
        file.write_all(b"x").unwrap();
        file.flush().unwrap();
        std::thread::sleep(Duration::from_millis(20));
    }
}
fn main() {
    let args: Vec<_> = env::args_os().skip(1).collect();
    let root = env::var_os("SHIELD_NATIVE_GATE").map(std::path::PathBuf::from);
    if args.first().is_some_and(|v| v == "--heartbeat-child") {
        heartbeat(root.as_deref().expect("自有夹具目录"), "child-heartbeat");
        return;
    }
    let executable = env::current_exe().unwrap();
    let keytool = executable.file_stem().is_some_and(|v| v == "keytool");
    let adapter = args.windows(2).any(|v| v[0] == "-jar" && Path::new(&v[1]).file_name().is_some_and(|n| n == "java-adapter.jar"));
    // 只在测试代理解码有限请求识别真实签名阶段；转发原字节，不把私有控制租约交给工具。
    let request = if adapter {
        let mut bytes = Vec::new();
        std::io::stdin().take(65537).read_to_end(&mut bytes).unwrap();
        assert!(bytes.len() <= 65536);
        Some(bytes)
    } else { None };
    let values = request.as_deref().map(decode_request);
    let signing = values.as_ref().is_some_and(|v| v.len() >= 3 && v[0] == "apksigner" && v[2] == "sign") || args.windows(3).any(|v| {
        v[0] == "-jar"
            && Path::new(&v[1])
                .file_name()
                .is_some_and(|n| n == "apksigner.jar")
            && v[2] == "sign"
    });
    if signing {
        if let Some(root) = &root {
            let mut child = no_window_command(&executable)
                .arg("--heartbeat-child")
                .env_remove("MOCIKA_SHIELD_KS_PASS")
                .env_remove("MOCIKA_SHIELD_KEY_PASS")
                .env_remove("MOCIKA_SHIELD_TOOL_KS_PASS")
                .env_remove("MOCIKA_SHIELD_TOOL_KEY_PASS")
                .spawn()
                .unwrap();
            heartbeat(root, "heartbeat");
            let _ = child.kill();
            let _ = child.wait();
        }
    }
    let real = env::var_os(if keytool {
        "SHIELD_NATIVE_KEYTOOL"
    } else {
        "SHIELD_NATIVE_JAVA"
    })
    .expect("显式真实工具");
    let mut command = no_window_command(real);
    // macOS 的系统 Java 可能是启动器；委托时恢复原 JAVA_HOME，不能误指向测试代理目录。
    match env::var_os("SHIELD_NATIVE_JAVA_HOME") {
        Some(home) => {
            command.env("JAVA_HOME", home);
        }
        None => {
            command.env_remove("JAVA_HOME");
        }
    }
    command.args(args);
    let status = if let Some(request) = request {
        let mut child = command.stdin(Stdio::piped()).spawn().unwrap();
        child.stdin.take().unwrap().write_all(&request).unwrap();
        child.wait().unwrap()
    } else { command.status().unwrap() };
    std::process::exit(status.code().unwrap_or(1));
}

fn decode_request(bytes: &[u8]) -> Vec<String> {
    assert_eq!(&bytes[..8], b"MSJA\0\0\0\x04");
    let number = |at| u32::from_be_bytes(bytes[at..at + 4].try_into().unwrap()) as usize;
    let count = number(8);
    assert!((1..=64).contains(&count));
    let mut at = 12;
    let mut values = Vec::new();
    for _ in 0..count {
        let size = number(at);
        at += 4;
        values.push(std::str::from_utf8(&bytes[at..at + size]).unwrap().to_owned());
        at += size;
    }
    assert_eq!(at, bytes.len());
    values
}

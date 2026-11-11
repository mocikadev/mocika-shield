//! 仅供原生任务测试：真实执行正式助手，再故意保持输出及后代活动以验证整组停止。
use std::{
    fs,
    io::{Read, Write},
    path::PathBuf,
    process::{Command, Stdio},
    time::Duration,
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

fn main() {
    let root = PathBuf::from(std::env::var_os("MOCIKA_TEST_GROUP_ROOT").unwrap());
    if std::env::args().nth(1).as_deref() == Some("--writer") {
        let mut byte = [0u8; 1];
        assert_eq!(std::io::stdin().read(&mut byte).unwrap(), 0);
        fs::write(root.join("adapter-stdin-empty"), b"yes").unwrap();
        let mut heartbeat = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(root.join("heartbeat"))
            .unwrap();
        for _ in 0..12000 {
            heartbeat.write_all(b".").unwrap();
            std::thread::sleep(Duration::from_millis(5));
        }
        return;
    }
    let java = std::env::var_os("MOCIKA_TEST_ADAPTER_JAVA").unwrap();
    let status = command(java)
        .args(std::env::args_os().skip(1))
        .stdin(Stdio::inherit())
        .status()
        .unwrap();
    assert!(status.success(), "正式助手必须真实执行成功后才进入故障窗口");
    fs::write(root.join("adapter-complete"), b"yes").unwrap();
    let mut writer = command(std::env::current_exe().unwrap())
        .arg("--writer")
        .stdin(Stdio::inherit())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    fs::write(root.join("child-pid"), writer.id().to_string()).unwrap();
    // stdout 保持打开，使核心仍在等 EOF；不能将主 Java 已退出冒充整组停止。
    let _ = writer.wait();
}

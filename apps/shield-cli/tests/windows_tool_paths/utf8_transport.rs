//! 原型请求编解码与临时 Java 助手生命周期，不负责证书和签名验收规则。
use super::bounded;
use std::{
    fs,
    path::PathBuf,
    process::{Command, Output},
};

const MAX_FRAME: usize = 65536;

pub(super) fn frame(args: &[&str]) -> Option<Vec<u8>> {
    if args.is_empty() || args.len() > 64 {
        return None;
    }
    let mut bytes = Vec::from(*b"UTF8");
    bytes.extend_from_slice(&1u32.to_be_bytes());
    bytes.extend_from_slice(&(args.len() as u32).to_be_bytes());
    for arg in args {
        if arg.len() > MAX_FRAME || bytes.len() + 4 + arg.len() > MAX_FRAME {
            return None;
        }
        bytes.extend_from_slice(&(arg.len() as u32).to_be_bytes());
        bytes.extend_from_slice(arg.as_bytes());
    }
    Some(bytes)
}

pub(super) struct Probe {
    _directory: tempfile::TempDir,
    pub(super) root: PathBuf,
    pub(super) java: PathBuf,
    pub(super) keytool: PathBuf,
}

impl Probe {
    pub(super) fn new() -> Self {
        let directory = tempfile::Builder::new()
            .prefix("shield-utf8-probe-")
            .tempdir()
            .unwrap();
        let root = shield_core::utils::strip_unc_prefix(directory.path());
        assert!(root.as_os_str().is_ascii(), "原型引导目录须为 ASCII");
        let keytool = shield_core::utils::find_keytool().unwrap();
        let bin = keytool.parent().unwrap();
        let java = bin.join(format!("java{}", std::env::consts::EXE_SUFFIX));
        let mut compile = shield_core::utils::no_window_command(
            bin.join(format!("javac{}", std::env::consts::EXE_SUFFIX)),
        );
        compile
            .args(["-encoding", "UTF-8", "-source", "8", "-target", "8", "-d"])
            .arg(&root);
        for (name, source) in [
            (
                "Utf8ArgumentProbe",
                include_str!("../../../../tests/fixtures/agent-signing/Utf8ArgumentProbe.java"),
            ),
            (
                "Utf8CertificateProbe",
                include_str!("../../../../tests/fixtures/agent-signing/Utf8CertificateProbe.java"),
            ),
        ] {
            let path = root.join(format!("{name}.java"));
            fs::write(&path, source).unwrap();
            compile.arg(path);
        }
        assert!(bounded(compile, &root).status.success(), "隔离原型编译失败");
        Self {
            _directory: directory,
            root,
            java,
            keytool,
        }
    }

    fn command(&self) -> Command {
        let mut command = shield_core::utils::java_command(&self.java);
        command
            .arg("-cp")
            .arg(&self.root)
            .arg("Utf8ArgumentProbe")
            .env_remove("JAVA_TOOL_OPTIONS")
            .env_remove("_JAVA_OPTIONS")
            .env_remove("JDK_JAVA_OPTIONS");
        command
    }

    pub(super) fn raw(&self, bytes: &[u8], password: &str) -> Output {
        // 只写固定合成参数，不把密码放文件；此原型不证明生产管道或取消语义。
        let request = self.root.join("request.bin");
        fs::write(&request, bytes).unwrap();
        let mut command = self.command();
        command
            .stdin(fs::File::open(request).unwrap())
            .env("SHIELD_PATH_PROBE_PASS", password);
        bounded(command, &self.root)
    }

    pub(super) fn invoke(&self, args: &[&str], password: &str) -> Output {
        self.raw(&frame(args).expect("测试请求不得超出帧预算"), password)
    }
}

#[test]
fn 帧预算包含头部和每个参数的长度且不截断字符() {
    assert!(frame(&[]).is_none());
    assert!(frame(&[""; 65]).is_none());
    assert!(frame(&[&"a".repeat(MAX_FRAME - 16)]).is_some());
    assert!(frame(&[&"a".repeat(MAX_FRAME - 15)]).is_none());
    assert!(frame(&[&"中".repeat(MAX_FRAME)]).is_none());
    assert_eq!(frame(&["中"]).unwrap().len(), 19);
}

//! 只读执行前计划回归；模拟工具不证明资源协议或私钥签名可用。
use serde_json::Value;
use std::process::{Command, Output};

fn command() -> Command {
    let mut cmd = shield_core::utils::no_window_command(env!("CARGO_BIN_EXE_shield"));
    cmd.env_remove("JAVA_HOME")
        .env_remove("MOCIKA_SHIELD_KS_PASS")
        .env_remove("MOCIKA_SHIELD_KEY_PASS");
    cmd
}

fn response(output: Output, exit: i32) -> Value {
    assert_eq!(output.status.code(), Some(exit), "{output:?}");
    assert!(output.stderr.is_empty(), "{output:?}");
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["schema_version"], 1);
    assert_eq!(value["operation"], "plan-protect");
    value
}

#[test]
fn 计划必须显式指定本地签名配置() {
    let output = command()
        .args(["plan-protect", "-i", "input.apk", "-o", "output.apk"])
        .output()
        .unwrap();
    assert_eq!(response(output, 1)["code"], "SIGNING_CONFIG_REQUIRED");
}

#[test]
fn 计划入口不接受密码参数() {
    let output = command()
        .args([
            "plan-protect",
            "-i",
            "input.apk",
            "-o",
            "output.apk",
            "--ks-pass",
            "不可发送的密码",
        ])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
}

#[cfg(unix)]
mod isolated {
    use super::*;
    use std::fs;
    use std::io::Write;
    use std::os::unix::fs::{symlink, PermissionsExt};
    use std::path::{Path, PathBuf};

    struct Fixture {
        root: tempfile::TempDir,
        input: PathBuf,
        output: PathBuf,
        config: PathBuf,
    }

    impl Fixture {
        fn new(abis: &[&str]) -> Self {
            let root = tempfile::tempdir().unwrap();
            let input = root.path().join("私有中文应用 空格.apk");
            let output = root.path().join("最终输出.apk");
            let config = root.path().join("shield-cli.toml");
            let mut zip = zip::ZipWriter::new(fs::File::create(&input).unwrap());
            for name in ["AndroidManifest.xml".to_owned(), "classes.dex".to_owned()]
                .into_iter()
                .chain(abis.iter().map(|abi| format!("lib/{abi}/libtest.so")))
            {
                zip.start_file(name, zip::write::SimpleFileOptions::default())
                    .unwrap();
                zip.write_all(b"fixture").unwrap();
            }
            zip.finish().unwrap();
            for name in [
                "apktool.jar",
                "apksigner.jar",
                "resources.zip",
                "credentials.p12",
            ] {
                fs::write(root.path().join(name), b"fixture").unwrap();
            }
            resources(root.path(), 21, true, false, &[]);
            fs::write(
                &config,
                r#"schema_version = 1
[protect]
apktool = "apktool.jar"
apksigner = "apksigner.jar"
resources = "resources.zip"
[sign]
keystore = "credentials.p12"
key_alias = "私有别名"
keystore_type = "pkcs12"
"#,
            )
            .unwrap();
            executable(
                root.path(),
                "java",
                r#"#!/bin/sh
for argument in "$@"; do
    if [ "$argument" = "-version" ]; then
        echo 'openjdk version "17.0.1"' >&2
        exit 0
    fi
done
echo 'Signer #1 certificate SHA-256 digest: aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa'
exit 0
"#,
            );
            executable(
                root.path(),
                "keytool",
                r#"#!/bin/sh
for argument in "$@"; do
    if [ "$argument" = "-storepass" ]; then exit 13; fi
done
if [ "$MOCIKA_SHIELD_TOOL_KS_PASS" != "$MOCIKA_SHIELD_KS_PASS" ]; then exit 14; fi
echo 'SHA256: aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa'
exit 0
"#,
            );
            Self {
                root,
                input,
                output,
                config,
            }
        }

        fn command(&self) -> Command {
            let mut cmd = command();
            cmd.env("PATH", self.root.path())
                .env("MOCIKA_SHIELD_KS_PASS", "fixture-super-secret")
                .arg("--config")
                .arg(&self.config)
                .arg("plan-protect")
                .arg("-i")
                .arg(&self.input)
                .arg("-o")
                .arg(&self.output);
            cmd
        }

        fn replace(&self, from: &str, to: &str) {
            let text = fs::read_to_string(&self.config).unwrap();
            fs::write(&self.config, text.replace(from, to)).unwrap();
        }
    }

    fn executable(root: &Path, name: &str, content: &str) {
        let path = root.join(name);
        fs::write(&path, content).unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
    }

    fn resources(root: &Path, api: u32, strict: bool, memory: bool, omit: &[&str]) {
        let metadata = serde_json::json!({
            "runtime_protocol": if memory { 5 } else { 4 }, "cache_schema": 1,
            "environment_policy": strict, "memory_dex": memory, "min_android_api": api,
            "memory_dex_min_api": 31, "stub_component_factory": "msk.f", "stub_application": "msk.t",
            "native_library": "libmocikashield.so", "native_name_placeholder": "mocikanativeslot",
            "native_name_length": 16, "native_name_scheme": 1,
        });
        let mut zip = zip::ZipWriter::new(fs::File::create(root.join("resources.zip")).unwrap());
        let mut files = vec![
            (
                "metadata.json".to_owned(),
                metadata.to_string().into_bytes(),
            ),
            ("stub-classes.dex".to_owned(), b"stub".to_vec()),
        ];
        files.extend(
            ["armeabi-v7a", "arm64-v8a", "x86", "x86_64"]
                .into_iter()
                .map(|abi| (format!("lib/{abi}/libmocikashield.so"), b"native".to_vec())),
        );
        for (name, data) in files {
            if omit.contains(&name.as_str()) {
                continue;
            }
            zip.start_file(name, zip::write::SimpleFileOptions::default())
                .unwrap();
            zip.write_all(&data).unwrap();
        }
        zip.finish().unwrap();
    }

    #[test]
    fn 计划无副作用且不发送材料路径或密码() {
        let fixture = Fixture::new(&[]);
        let before = fs::read(&fixture.input).unwrap();
        let value = response(fixture.command().output().unwrap(), 0);
        assert_eq!(value["status"], "ready");
        assert_eq!(value["data"]["execution_ready"], false);
        assert_eq!(value["data"]["private_key_access_checked"], false);
        assert_eq!(value["data"]["resource_protocol_checked"], true);
        assert_eq!(value["data"]["plan_schema_version"], 2);
        assert_eq!(value["data"]["plan_digest"].as_str().unwrap().len(), 64);
        let serialized = value.to_string();
        for text in [
            "fixture-super-secret",
            "私有中文应用",
            "私有别名",
            fixture.root.path().to_str().unwrap(),
        ] {
            assert!(!serialized.contains(text));
        }
        assert_eq!(before, fs::read(&fixture.input).unwrap());
        assert!(!fixture.output.exists());
        let second = response(fixture.command().output().unwrap(), 0);
        assert_eq!(value["data"]["plan_digest"], second["data"]["plan_digest"]);
    }

    #[test]
    fn 输入内容变化会改变计划摘要() {
        let fixture = Fixture::new(&[]);
        let first = response(fixture.command().output().unwrap(), 0);
        let mut zip = zip::ZipWriter::new_append(
            fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open(&fixture.input)
                .unwrap(),
        )
        .unwrap();
        zip.start_file("assets/new", zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(b"changed").unwrap();
        zip.finish().unwrap();
        let second = response(fixture.command().output().unwrap(), 0);
        assert_ne!(first["data"]["plan_digest"], second["data"]["plan_digest"]);
        assert_ne!(
            first["data"]["input_sha256"],
            second["data"]["input_sha256"]
        );
    }

    #[test]
    fn 已有输出和同输入不能覆盖() {
        let fixture = Fixture::new(&[]);
        fs::write(&fixture.output, b"keep").unwrap();
        assert_eq!(
            response(fixture.command().output().unwrap(), 1)["code"],
            "OUTPUT_EXISTS"
        );
        assert_eq!(fs::read(&fixture.output).unwrap(), b"keep");
        fs::remove_file(&fixture.output).unwrap();
        fs::create_dir(&fixture.output).unwrap();
        assert_eq!(
            response(fixture.command().output().unwrap(), 1)["code"],
            "OUTPUT_EXISTS"
        );
        // 另起命令，避免重复 -o 只验证到 clap 而没有检查实际路径。
        let mut cmd = command();
        cmd.env("MOCIKA_SHIELD_KS_PASS", "fixture-super-secret")
            .arg("--config")
            .arg(&fixture.config)
            .arg("plan-protect")
            .arg("-i")
            .arg(&fixture.input)
            .arg("-o")
            .arg(&fixture.input);
        assert_eq!(response(cmd.output().unwrap(), 1)["code"], "OUTPUT_EXISTS");
    }

    #[test]
    fn 拒绝软硬链接及悬空链接输出() {
        for mode in ["symlink", "hardlink", "dangling"] {
            let fixture = Fixture::new(&[]);
            match mode {
                "hardlink" => fs::hard_link(&fixture.input, &fixture.output).unwrap(),
                "dangling" => symlink(fixture.root.path().join("不存在"), &fixture.output).unwrap(),
                _ => symlink(&fixture.input, &fixture.output).unwrap(),
            }
            assert_eq!(
                response(fixture.command().output().unwrap(), 1)["code"],
                "OUTPUT_EXISTS"
            );
            assert!(fs::symlink_metadata(&fixture.output).is_ok());
        }
    }

    #[test]
    fn 缺少宿主凭据不回退到对话或命令参数() {
        let fixture = Fixture::new(&[]);
        let value = response(
            fixture
                .command()
                .env_remove("MOCIKA_SHIELD_KS_PASS")
                .output()
                .unwrap(),
            1,
        );
        assert_eq!(value["code"], "CREDENTIAL_UNAVAILABLE");
    }

    #[test]
    fn 非法配置仅返回固定错误() {
        let fixture = Fixture::new(&[]);
        fs::write(
            &fixture.config,
            "schema_version = 1\npassword = 'fixture-super-secret'\n",
        )
        .unwrap();
        let value = response(fixture.command().output().unwrap(), 1);
        assert_eq!(value["code"], "CONFIG_INVALID");
        assert!(!value.to_string().contains("fixture-super-secret"));
    }

    #[test]
    fn 证书不一致必须阻断() {
        let fixture = Fixture::new(&[]);
        let keytool = fs::read_to_string(fixture.root.path().join("keytool")).unwrap();
        executable(
            fixture.root.path(),
            "keytool",
            &keytool.replace(&"a".repeat(64), &"b".repeat(64)),
        );
        let value = response(fixture.command().output().unwrap(), 1);
        assert_eq!(value["status"], "blocked");
        assert!(value["data"]["preflight"]["checks"]
            .as_array()
            .unwrap()
            .iter()
            .any(|check| check["code"] == "certificate_mismatch"));
        assert!(!fixture.output.exists());
    }

    #[test]
    fn 混合旧架构须逐次显式授权() {
        let fixture = Fixture::new(&["arm64-v8a", "armeabi"]);
        assert_eq!(
            response(fixture.command().output().unwrap(), 1)["code"],
            "UNSUPPORTED_ABI"
        );
        let value = response(
            fixture
                .command()
                .args(["--exclude-abis", "armeabi"])
                .output()
                .unwrap(),
            0,
        );
        assert_eq!(value["status"], "warning");
        assert_eq!(
            value["data"]["excluded_abis"],
            serde_json::json!(["armeabi"])
        );
        assert_eq!(
            response(fixture.command().output().unwrap(), 1)["code"],
            "UNSUPPORTED_ABI"
        );
    }

    #[test]
    fn 不创建缺失父目录且检查_v4_旁文件冲突() {
        let fixture = Fixture::new(&[]);
        fixture.replace("[sign]", "[sign]\nv4 = true");
        fs::write(fixture.root.path().join("最终输出.apk.idsig"), b"keep").unwrap();
        assert_eq!(
            response(fixture.command().output().unwrap(), 1)["code"],
            "OUTPUT_EXISTS"
        );
        fs::remove_file(fixture.root.path().join("最终输出.apk.idsig")).unwrap();
        let parent = fixture.root.path().join("缺失父目录");
        let output = command()
            .env("MOCIKA_SHIELD_KS_PASS", "fixture-super-secret")
            .arg("--config")
            .arg(&fixture.config)
            .arg("plan-protect")
            .arg("-i")
            .arg(&fixture.input)
            .arg("-o")
            .arg(parent.join("output.apk"))
            .output()
            .unwrap();
        assert_eq!(response(output, 1)["code"], "OUTPUT_PARENT_UNAVAILABLE");
        assert!(!parent.exists());
    }

    #[test]
    fn 工具执行期间输入变化或输出出现会拒绝计划() {
        for kind in ["input", "output"] {
            let fixture = Fixture::new(&[]);
            let path = fixture.root.path().join("keytool");
            let program = fs::read_to_string(&path).unwrap();
            executable(
                fixture.root.path(),
                "keytool",
                &program.replace(
                    "echo 'SHA256:",
                    "printf 'changed' >> \"$SHIELD_FIXTURE_CHANGE_TARGET\"\necho 'SHA256:",
                ),
            );
            let target = if kind == "input" {
                &fixture.input
            } else {
                &fixture.output
            };
            let output = fixture
                .command()
                .env("SHIELD_FIXTURE_CHANGE_TARGET", target)
                .output()
                .unwrap();
            let value = response(output, 1);
            assert_eq!(
                value["code"],
                if kind == "input" {
                    "PLAN_SOURCE_CHANGED"
                } else {
                    "OUTPUT_EXISTS"
                }
            );
        }
    }

    #[test]
    fn 配置链接不改变相对材料目录() {
        let fixture = Fixture::new(&[]);
        let elsewhere = fixture.root.path().join("外部配置目录");
        fs::create_dir(&elsewhere).unwrap();
        let target = elsewhere.join("actual.toml");
        fs::rename(&fixture.config, &target).unwrap();
        symlink(&target, &fixture.config).unwrap();
        assert_eq!(
            response(fixture.command().output().unwrap(), 0)["status"],
            "ready"
        );
    }

    #[test]
    fn 配置和签名策略变化均影响计划身份() {
        let fixture = Fixture::new(&[]);
        let first = response(fixture.command().output().unwrap(), 0);
        fixture.replace("[protect]", "[protect]\nenvironment_policy = 'strict'");
        let second = response(fixture.command().output().unwrap(), 0);
        assert_eq!(second["data"]["environment_policy"], "strict");
        assert_ne!(first["data"]["plan_digest"], second["data"]["plan_digest"]);
        fixture.replace("[sign]", "[sign]\nv4 = true");
        let third = response(fixture.command().output().unwrap(), 0);
        assert_eq!(third["data"]["output_count"], 2);
        assert_ne!(second["data"]["plan_digest"], third["data"]["plan_digest"]);
    }

    #[test]
    fn 显式输入输出覆盖旧配置默认值且工具冲突不猜测() {
        let fixture = Fixture::new(&[]);
        for section in ["[protect]", "[sign]"] {
            fixture.replace(
                section,
                &format!("{section}\ninput = '不使用的输入.apk'\noutput = '不使用的输出.apk'"),
            );
        }
        assert_eq!(
            response(fixture.command().output().unwrap(), 0)["status"],
            "ready"
        );
        fs::write(fixture.root.path().join("other.jar"), b"fixture").unwrap();
        fixture.replace("[sign]", "[sign]\napksigner = 'other.jar'");
        assert_eq!(
            response(fixture.command().output().unwrap(), 1)["code"],
            "INCONSISTENT_TOOL_CONFIG"
        );
    }

    #[test]
    fn 核心签名参数不包含密码明文() {
        let fixture = Fixture::new(&[]);
        executable(
            fixture.root.path(),
            "java",
            r#"#!/bin/sh
previous=''
output=''
for argument in "$@"; do
    if [ "$argument" = '-version' ]; then echo 'openjdk version "17.0.1"' >&2; exit 0; fi
    case "$argument" in pass:*|fixture-super-secret|fixture-key-secret) exit 71;; esac
    if [ "$previous" = '--out' ]; then output="$argument"; fi
    previous="$argument"
done
if [ "$MOCIKA_SHIELD_TOOL_KS_PASS" != 'fixture-super-secret' ]; then exit 72; fi
if [ "$MOCIKA_SHIELD_TOOL_KEY_PASS" != 'fixture-key-secret' ]; then exit 73; fi
printf '仅验证工具传参，不是真实签名 APK' > "$output"
"#,
        );
        let output = command()
            .env("PATH", fixture.root.path())
            .env("MOCIKA_SHIELD_KS_PASS", "fixture-super-secret")
            .env("MOCIKA_SHIELD_KEY_PASS", "fixture-key-secret")
            .arg("--config")
            .arg(&fixture.config)
            .arg("sign")
            .arg("--json")
            .arg("-i")
            .arg(&fixture.input)
            .arg("-o")
            .arg(&fixture.output)
            .output()
            .unwrap();
        assert!(output.status.success());
        assert!(fixture.output.is_file());
        let text = String::from_utf8(output.stdout).unwrap();
        assert!(!text.contains("fixture-super-secret"));
        assert!(!text.contains("fixture-key-secret"));
    }

    #[test]
    fn 资源模式必须匹配且工控预检与计划绑定同一模式() {
        let fixture = Fixture::new(&["armeabi-v7a", "arm64-v8a"]);
        resources(fixture.root.path(), 19, true, false, &[]);
        assert_eq!(
            response(fixture.command().output().unwrap(), 1)["code"],
            "RUNTIME_RESOURCE_INVALID"
        );
        let value = response(
            fixture
                .command()
                .args(["--runtime-profile", "android-api19"])
                .output()
                .unwrap(),
            0,
        );
        assert_eq!(value["data"]["runtime_profile"], "android-api19");
        assert_eq!(
            value["data"]["preflight"]["runtime_profile"],
            "android-api19"
        );
        assert_eq!(value["data"]["resource_protocol_checked"], true);
        assert_eq!(value["data"]["execution_ready"], false);
        resources(fixture.root.path(), 21, true, false, &[]);
        assert_eq!(
            response(
                fixture
                    .command()
                    .args(["--runtime-profile", "android-api19"])
                    .output()
                    .unwrap(),
                1
            )["code"],
            "RUNTIME_RESOURCE_INVALID"
        );
    }

    #[test]
    fn 坏资源内存候选或不支持的严格策略不能生成计划() {
        let fixture = Fixture::new(&[]);
        fs::write(fixture.root.path().join("resources.zip"), b"bad").unwrap();
        assert_eq!(
            response(fixture.command().output().unwrap(), 1)["code"],
            "RUNTIME_RESOURCE_INVALID"
        );
        resources(fixture.root.path(), 21, true, true, &[]);
        assert_eq!(
            response(fixture.command().output().unwrap(), 1)["code"],
            "RUNTIME_RESOURCE_INVALID"
        );
        fixture.replace("[protect]", "[protect]\nenvironment_policy = 'strict'");
        resources(fixture.root.path(), 21, false, false, &[]);
        assert_eq!(
            response(fixture.command().output().unwrap(), 1)["code"],
            "RUNTIME_RESOURCE_INVALID"
        );
    }

    #[test]
    fn 所需规范库缺失不能通过但不要求无关架构() {
        let fixture = Fixture::new(&[]);
        resources(
            fixture.root.path(),
            21,
            true,
            false,
            &["lib/x86/libmocikashield.so"],
        );
        assert_eq!(
            response(fixture.command().output().unwrap(), 1)["code"],
            "RUNTIME_RESOURCE_INVALID"
        );
        let fixture = Fixture::new(&["armeabi-v7a"]);
        resources(
            fixture.root.path(),
            21,
            true,
            false,
            &["lib/x86/libmocikashield.so"],
        );
        assert_eq!(
            response(fixture.command().output().unwrap(), 0)["data"]["resource_protocol_checked"],
            true
        );
    }

    #[test]
    fn 工控计划仍要求_v7a_并保留单次旧架构授权() {
        let fixture = Fixture::new(&["arm64-v8a"]);
        resources(fixture.root.path(), 19, true, false, &[]);
        let value = response(
            fixture
                .command()
                .args(["--runtime-profile", "android-api19"])
                .output()
                .unwrap(),
            1,
        );
        assert_eq!(value["status"], "blocked");
        assert_eq!(value["data"]["resource_protocol_checked"], false);
        let fixture = Fixture::new(&["armeabi-v7a", "armeabi"]);
        resources(fixture.root.path(), 19, true, false, &[]);
        assert_eq!(
            response(
                fixture
                    .command()
                    .args(["--runtime-profile", "android-api19"])
                    .output()
                    .unwrap(),
                1
            )["code"],
            "UNSUPPORTED_ABI"
        );
        let value = response(
            fixture
                .command()
                .args([
                    "--runtime-profile",
                    "android-api19",
                    "--exclude-abis",
                    "armeabi",
                ])
                .output()
                .unwrap(),
            0,
        );
        assert_eq!(value["status"], "warning");
        assert_eq!(value["data"]["resource_protocol_checked"], true);
    }
}

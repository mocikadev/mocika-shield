//! 两个固定 JAR 入口的隔离契约；不代表完整 APK 或公开任务链路验收。
use super::*;
use crate::java_adapter::jars::{JarOutput, JarTool};

fn adapter() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tools/java-adapter/build/java-adapter.jar")
}

fn fixture_jar(root: &Path, tool: JarTool, manifest: &str) -> std::path::PathBuf {
    let (package, class) = match tool {
        JarTool::Apksigner => ("com.android.apksigner", "ApkSignerTool"),
        JarTool::Apktool => ("brut.apktool", "Main"),
    };
    let source = include_str!("../../../../tests/fixtures/agent-signing/FixedJarProbe.java.in")
        .replace("TOOL_PACKAGE", package)
        .replace("TOOL_MAIN", class);
    let file = format!("{class}.java");
    fs::write(root.join(&file), source).unwrap();
    let keytool = crate::utils::find_keytool().unwrap();
    let javac = keytool
        .parent()
        .unwrap()
        .join(format!("javac{}", std::env::consts::EXE_SUFFIX));
    assert!(
        crate::utils::no_window_command(javac)
            .current_dir(root)
            .args([
                "-encoding",
                "UTF-8",
                "-source",
                "8",
                "-target",
                "8",
                "-d",
                ".",
                &file
            ])
            .output()
            .unwrap()
            .status
            .success(),
        "固定入口测试夹具编译失败"
    );
    let path = root.join(format!("中文 工具-{class}.jar"));
    let mut zip = zip::ZipWriter::new(fs::File::create(&path).unwrap());
    let options = zip::write::SimpleFileOptions::default();
    zip.start_file("META-INF/MANIFEST.MF", options).unwrap();
    zip.write_all(format!("Manifest-Version: 1.0\r\nMain-Class: {manifest}\r\n\r\n").as_bytes())
        .unwrap();
    // 匿名线程处理类也随夹具打包；正式适配资源不得夹带这些测试类。
    for suffix in ["", "$1"] {
        let name = format!("{}/{class}{suffix}.class", package.replace('.', "/"));
        zip.start_file(&name, options).unwrap();
        zip.write_all(&fs::read(root.join(name)).unwrap()).unwrap();
    }
    zip.start_file("probe.txt", options).unwrap();
    zip.write_all("中文资源一致".as_bytes()).unwrap();
    zip.finish().unwrap();
    path
}

fn run(
    context: &JavaToolContext,
    java: &Path,
    tool: JarTool,
    jar: &Path,
    args: &[&str],
) -> JarOutput {
    let command = context.prepare_jar(java, tool, jar, args).unwrap();
    for name in [
        crate::java_process::KS_PASS_ENV,
        crate::java_process::KEY_PASS_ENV,
    ] {
        assert!(
            command
                .get_envs()
                .any(|(key, value)| key == name && value.is_none()),
            "无证书用例必须明确清除继承密码"
        );
    }
    assert!(
        command.get_args().all(|arg| arg.is_ascii()),
        "原生 argv 不得携带业务参数"
    );
    JavaToolContext::collect_jar(command, tool).unwrap()
}

#[test]
#[ignore = "需预构建正式 JAR 和显式 Java；验证固定类、资源、EOF 与双流预算"]
fn 固定jar入口保持中文参数资源与双流退出语义() {
    let root = private_root();
    let workspace = root.path().join("中文 私有目录");
    create_workspace(&workspace).unwrap();
    let context = JavaToolContext::new(&adapter(), &workspace).unwrap();
    let java = crate::utils::find_java().unwrap();
    let input = root.path().join("中文 输入😀.txt");
    fs::write(&input, "文件实际内容").unwrap();
    for (tool, entry) in [
        (JarTool::Apksigner, "com.android.apksigner.ApkSignerTool"),
        (JarTool::Apktool, "brut.apktool.Main"),
    ] {
        let jar = fixture_jar(root.path(), tool, entry);
        let output = run(
            &context,
            &java,
            tool,
            &jar,
            &[
                "roundtrip",
                "中文 空格&引号\"反斜杠\\",
                "",
                "补充😀",
                input.to_str().unwrap(),
            ],
        );
        assert!(output.status.success());
        assert_eq!(output.stdout.bytes, "中文资源一致".as_bytes());
        assert_eq!(output.stderr.bytes, "中文错误流".as_bytes());
        assert!(!output.stdout.truncated && !output.stderr.truncated);
        let late = run(&context, &java, tool, &jar, &["late"]);
        assert!(late.status.success());
        assert_eq!(
            late.stdout.bytes,
            "中文资源一致".as_bytes(),
            "主函数返回后不能关闭后代线程的加载器"
        );
        let exit = run(&context, &java, tool, &jar, &["exit"]);
        assert_eq!(exit.status.code(), Some(17), "原工具主动退出码必须保留");
        let thrown = run(&context, &java, tool, &jar, &["throw"]);
        assert_eq!(thrown.status.code(), Some(1));
        assert_eq!(
            std::str::from_utf8(&thrown.stderr.bytes).unwrap().trim(),
            "JAVA_ADAPTER_TOOL_EXCEPTION"
        );
        assert!(thrown.stdout.bytes.is_empty());
        let flood = run(&context, &java, tool, &jar, &["flood"]);
        assert!(flood.status.success());
        for captured in [&flood.stdout, &flood.stderr] {
            assert_eq!(captured.bytes, vec![b'x'; 65536]);
            assert!(captured.truncated);
        }
        let wrong = fixture_jar(root.path(), tool, "untrusted.AnyMain");
        let rejected = run(&context, &java, tool, &wrong, &["exit"]);
        assert_eq!(rejected.status.code(), Some(3));
        assert!(rejected.stdout.bytes.is_empty());
        assert_eq!(
            std::str::from_utf8(&rejected.stderr.bytes).unwrap().trim(),
            "JAVA_ADAPTER_JAR_INVALID"
        );
        assert!(context
            .prepare_jar(&java, tool, &wrong, &["bad\0argument"])
            .is_err());
    }
    // 绕过 Rust 预检单独验证正式助手自身的边界；损坏尾随不能先执行工具。
    let jar = fixture_jar(
        root.path(),
        JarTool::Apksigner,
        "com.android.apksigner.ApkSignerTool",
    );
    for values in [
        vec!["apksigner", jar.to_str().unwrap()],
        vec!["apksigner", jar.to_str().unwrap(), "bad\0argument"],
        vec!["arbitrary_class", jar.to_str().unwrap(), "exit"],
    ] {
        let output = context
            .prepare_request(&java, &values)
            .unwrap()
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
    }
    let mut command = context
        .prepare_request(&java, &["apksigner", jar.to_str().unwrap(), "exit"])
        .unwrap();
    let directory = command.get_current_dir().unwrap().join("trailing");
    create_workspace(&directory).unwrap();
    let mut frame = request::encode(&["apksigner", jar.to_str().unwrap(), "exit"]).unwrap();
    frame.push(0);
    let output = command
        .stdin(request::input(&directory, &frame).unwrap())
        .output()
        .unwrap();
    assert_eq!(
        output.status.code(),
        Some(2),
        "确认 EOF 前不能执行主动退出工具"
    );
    assert!(output.stdout.is_empty());
    println!("固定 JAR 契约：双工具参数／资源／EOF／双流一致，延迟资源读取、退出码、异常脱敏及日志截断均通过；非完整 APK 验收");
}

#[test]
#[ignore = "需两个仓库原始 JAR 和显式 Java；版本及错误退出对照，不是完整签名／加固"]
fn 正式固定入口与两个原工具版本及错误退出一致() {
    let root = private_root();
    let workspace = root.path().join("中文 工作目录");
    create_workspace(&workspace).unwrap();
    let context = JavaToolContext::new(&adapter(), &workspace).unwrap();
    let java = crate::utils::find_java().unwrap();
    for (tool, original, argument) in [
        (
            JarTool::Apksigner,
            crate::utils::find_apksigner().unwrap(),
            "version",
        ),
        (
            JarTool::Apktool,
            crate::utils::find_apktool().unwrap(),
            "--version",
        ),
    ] {
        let jar = root.path().join("中文 选定工具.jar");
        fs::copy(&original, &jar).unwrap();
        let version = match tool {
            JarTool::Apksigner => context.query_apksigner_version(&java, &jar),
            JarTool::Apktool => context.query_apktool_version(&java, &jar),
        }
        .unwrap();
        let baseline = java_command(&java)
            .arg("-jar")
            .arg(&original)
            .arg(argument)
            .output()
            .unwrap();
        assert!(baseline.status.success());
        assert_eq!(
            version,
            std::str::from_utf8(&baseline.stdout).unwrap().trim()
        );
        for args in [[argument], ["--invalid-adapter-test-option"]] {
            let output = run(&context, &java, tool, &jar, &args);
            let baseline = java_command(&java)
                .arg("-jar")
                .arg(&original)
                .args(args)
                .output()
                .unwrap();
            assert_eq!(output.status.code(), baseline.status.code());
            assert_eq!(output.stdout.bytes, baseline.stdout);
            assert_eq!(output.stderr.bytes, baseline.stderr);
            assert!(!output.stdout.truncated && !output.stderr.truncated);
        }
        let error = context.query_apksigner_version(&java, &jar).err();
        if matches!(tool, JarTool::Apktool) {
            let error = error.unwrap();
            let diagnostic = Diagnostic::from_error(&error);
            assert_eq!(
                diagnostic.tool,
                Some(crate::diagnostic::ToolName::Apksigner)
            );
            assert_eq!(diagnostic.exit_code, Some(3));
            assert!(!error.to_string().contains("JAVA_ADAPTER_JAR_INVALID"));
            assert!(!error.to_string().contains("中文"));
        }
        println!("原工具固定入口：版本 {version}、自定义中文 JAR 路径及无效选项退出／双流对照通过；完整 APK 流程尚未验证");
    }
}

#[test]
#[ignore = "需正式 JAR 和显式 JDK；用固定入口夹具验证完整验签结果预算，不是实际 APK 验签"]
fn 上下文验签拒绝截断及错编码而不返回部分指纹() {
    let root = private_root();
    let context = JavaToolContext::new(&adapter(), root.path()).unwrap();
    let java = crate::utils::find_java().unwrap();
    let jar = fixture_jar(
        root.path(),
        JarTool::Apksigner,
        "com.android.apksigner.ApkSignerTool",
    );
    let input = root.path().join("verification-budget.txt");
    for (mode, code) in [
        ("stdout-flood", FailureCode::ToolProcessFailed),
        ("stderr-flood", FailureCode::ToolProcessFailed),
        ("invalid-utf8", FailureCode::ToolOutputEncodingInvalid),
    ] {
        fs::write(&input, mode).unwrap();
        let error = crate::extract_apk_cert_fingerprint_with_context(&context, &java, &input, &jar)
            .unwrap_err();
        let diagnostic = Diagnostic::from_error(&error);
        assert_eq!(diagnostic.code, code);
        assert_eq!(
            diagnostic.tool,
            Some(crate::diagnostic::ToolName::Apksigner)
        );
        assert_eq!(diagnostic.exit_code, Some(0));
        assert!(
            !format!("{error:#}").contains(&"1".repeat(64)),
            "不能回传部分有效指纹"
        );
        assert!(!format!("{error:#}").contains(input.to_str().unwrap()));
        println!("验签预算分支：{mode}，整体拒绝且无部分指纹");
    }
    println!("验签预算回归：stdout／stderr 超量和 stdout 非法 UTF-8 均整体拒绝，未返回部分指纹或原始日志");
}

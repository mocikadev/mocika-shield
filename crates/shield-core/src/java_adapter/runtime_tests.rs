//! 显式运行真实 JDK 的引导与已知限制对照，不与普通结构单元混同。
use super::*;

#[test]
#[ignore = "需预构建正式 JAR；验证 Java 8／17 的严格解码和未知操作拒绝"]
fn 正式资源拒绝损坏请求及未开放操作() {
    let root = private_root();
    let jar = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tools/java-adapter/build/java-adapter.jar");
    let context = JavaToolContext::new(&jar, root.path()).unwrap();
    let java = crate::utils::find_java().unwrap();
    let valid = request::encode(&["hello"]).unwrap();
    let mut magic = valid.clone();
    magic[0] ^= 1;
    let mut version = valid.clone();
    version[4..8].copy_from_slice(&(PROTOCOL + 1).to_be_bytes());
    let mut count = valid.clone();
    count[11] = 65;
    let mut length = valid.clone();
    length[12..16].copy_from_slice(&65536u32.to_be_bytes());
    let mut utf8 = valid.clone();
    utf8[16] = 0xff;
    let mut truncated = valid.clone();
    truncated.pop();
    let mut trailing = valid;
    trailing.push(0);
    for bytes in [
        magic,
        version,
        count,
        length,
        utf8,
        truncated,
        trailing,
        request::encode(&["certificate"]).unwrap(),
    ] {
        let mut command = context.prepare(&java).unwrap();
        let directory = command.get_current_dir().unwrap().join("negative");
        create_workspace(&directory).unwrap();
        command.stdin(request::input(&directory, &bytes).unwrap());
        let output = command.output().unwrap();
        assert_eq!(output.status.code(), Some(2), "损坏请求或未接业务必须拒绝");
        assert!(output.stdout.is_empty());
        assert!(!directory.join("landed.txt").exists());
    }
    println!(
        "正式请求拒绝结果：{}",
        json!({"rejected":8,"arbitrary_operations_enabled":false})
    );
}

#[test]
#[ignore = "需预构建正式适配 JAR；显式选择 Java 8／17 验证中文引导，不是业务写入"]
fn 正式资源握手及中文工作目录实际落点() {
    assert!(
        ["JAVA_TOOL_OPTIONS", "_JAVA_OPTIONS", "JDK_JAVA_OPTIONS"]
            .iter()
            .all(|name| std::env::var_os(name).is_none_or(|value| value.is_empty())),
        "原生夹具不接受外来 JVM 选项"
    );
    let root = tempfile::Builder::new()
        .prefix("adapter-runtime-")
        .tempdir()
        .unwrap();
    let compile_root = strip_unc_prefix(root.path());
    assert!(
        compile_root.as_os_str().is_ascii(),
        "编译夹具需要独立 ASCII 临时根"
    );
    let source_jar = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tools/java-adapter/build/java-adapter.jar");
    let java = crate::utils::find_java().unwrap();
    let javac = crate::utils::find_keytool()
        .unwrap()
        .parent()
        .unwrap()
        .join(format!("javac{}", std::env::consts::EXE_SUFFIX));
    fs::copy(&source_jar, compile_root.join("adapter.jar")).unwrap();
    fs::write(
        compile_root.join("BootstrapProbe.java"),
        include_str!("../../../../tests/fixtures/agent-signing/BootstrapProbe.java"),
    )
    .unwrap();
    let mut compile = crate::utils::no_window_command(javac);
    let output = compile
        .current_dir(&compile_root)
        .args([
            "-encoding",
            "UTF-8",
            "-source",
            "8",
            "-target",
            "8",
            "-cp",
            "adapter.jar",
            "-d",
            ".",
            "BootstrapProbe.java",
        ])
        .output()
        .unwrap();
    assert!(output.status.success(), "独立引导夹具编译失败");
    for name in ["ascii space", "中文 目录"] {
        let workspace = compile_root.join(name);
        create_workspace(&workspace).unwrap();
        let context = JavaToolContext::new(&source_jar, &workspace).unwrap();
        if context.verify_runtime(&java).is_err() {
            // 仅失败测试的有限诊断，不作为生产失败后的自动重试；仍保留严格失败断言。
            let mut command = context.prepare(&java).unwrap();
            let cwd = command.get_current_dir().unwrap().to_owned();
            let output = command.output().unwrap();
            let error = String::from_utf8_lossy(&output.stderr);
            let settings = java_command(&java)
                .current_dir(&cwd)
                .args(["-XshowSettings:properties", "-version"])
                .output()
                .unwrap();
            let properties = String::from_utf8_lossy(&settings.stderr);
            let user_dir = properties
                .lines()
                .find_map(|line| line.trim().strip_prefix("user.dir = "));
            println!(
                "安全短名引导失败分类：{}",
                json!({
                    "case":name,"exit":output.status.code(),"cwd_ascii":cwd.as_os_str().is_ascii(),
                    "percent_decode_failed":error.contains("Error decoding percent encoded characters"),
                    "class_not_found":error.contains("ClassNotFoundException"),
                    "cannot_access_jar":error.contains("Unable to access jarfile"),
                    "stdout_empty":output.stdout.is_empty(),"settings_ok":settings.status.success(),
                    "user_dir_ascii":user_dir.is_some_and(str::is_ascii),
                    "user_dir_has_supplement":user_dir.is_some_and(|text| text.chars().any(|ch| ch as u32 > 0xffff))
                })
            );
            panic!("当前原生引导必须通过，不能以未支持分支绕过 Windows 短名验收");
        }
        let invocation = context.prepare(&java).unwrap();
        let directory = invocation.get_current_dir().unwrap();
        let fixture_path = directory.join("dev/mocika/shield/tools");
        fs::create_dir_all(&fixture_path).unwrap();
        fs::copy(
            compile_root.join("dev/mocika/shield/tools/BootstrapProbe.class"),
            fixture_path.join("BootstrapProbe.class"),
        )
        .unwrap();
        let input = directory.join("中文 输入.txt");
        fs::write(&input, "引导读取一致").unwrap();
        let request_dir = directory.join("probe");
        create_workspace(&request_dir).unwrap();
        let request = request::input(
            &request_dir,
            &request::encode(&[
                "bootstrap",
                directory.to_str().unwrap(),
                input.to_str().unwrap(),
            ])
            .unwrap(),
        )
        .unwrap();
        let mut command = java_command(&java);
        let classpath = if cfg!(windows) {
            "java-adapter.jar;."
        } else {
            "java-adapter.jar:."
        };
        let output = command
            .current_dir(directory)
            .args(["-cp", classpath, "dev.mocika.shield.tools.BootstrapProbe"])
            .stdin(request)
            .output()
            .unwrap();
        assert!(output.status.success(), "引导实际落点夹具失败");
        let result: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        println!("正式引导结果：{}", json!({"case":name,"result":result}));
        for field in [
            "cwd_matches",
            "input_matches",
            "output_matches",
            "user_dir_matches",
        ] {
            assert_eq!(result[field], true, "实际落点未匹配：{field}");
        }
        assert_eq!(
            fs::read_to_string(directory.join("landed.txt")).unwrap(),
            "引导输出一致"
        );
        assert!(workspace.exists(), "上下文不能删除借用根");
    }
}

#[cfg(windows)]
#[test]
#[ignore = "真实 Java 8／17 已知限制对照：短名不是补充字符 JAR 引导兼容证明"]
fn 短名通过身份核对不代表jdk能够引导补充字符目录() {
    let root = private_root();
    let workspace = root.path().join("补充😀");
    create_workspace(&workspace).unwrap();
    let source = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tools/java-adapter/build/java-adapter.jar");
    let java = crate::utils::find_java().unwrap();
    let context = JavaToolContext::new(&source, &workspace).unwrap();
    assert!(context.prepare(&java).is_err(), "正式入口必须在启动前拒绝");
    assert_eq!(fs::read_dir(&workspace).unwrap().count(), 0);
    // 以下只保留既有失败的独立对照，不是生产自动重试或绕过正式边界。
    let alias = crate::java_directory::ascii_directory(&workspace).unwrap();
    assert!(crate::java_directory::same_directory(&workspace, &alias).unwrap());
    fs::copy(&source, workspace.join("java-adapter.jar")).unwrap();
    let input = request::input(&workspace, &request::encode(&["hello"]).unwrap()).unwrap();
    let output = java_command(&java)
        .current_dir(&alias)
        .args(["-jar", "java-adapter.jar"])
        .stdin(input)
        .output()
        .unwrap();
    let error = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(
        error.contains("Error decoding percent encoded characters"),
        "只接受已定位的 JVM 限制，未知失败必须重新核对"
    );
    println!(
        "补充字符限制对照：{}",
        json!({"cwd_ascii":alias.as_os_str().is_ascii(),
        "same_directory":true,"percent_decode_failed":true,"supported":false,
        "production_rejected_before_spawn":true,"business_started":false})
    );
}

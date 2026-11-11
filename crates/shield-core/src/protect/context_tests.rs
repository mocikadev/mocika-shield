use super::context_assertions as assertions;
use super::context_fixture as fixture;
use super::*;
use fixture::Fixture;
use std::{path::Path, sync::Mutex};

fn resources(api19: bool) -> PathBuf {
    PathBuf::from(
        std::env::var_os(if api19 {
            "SHIELD_TEST_API19_RESOURCES"
        } else {
            "SHIELD_TEST_STANDARD_RESOURCES"
        })
        .expect("显式指定匹配的真实 Android 壳资源"),
    )
}

#[test]
#[ignore = "需正式适配 JAR、两套真实壳资源、自有 DEX APK 及显式 JDK／SDK"]
fn 双资源完整加固证书绑定与原生库打包() {
    let fixture = Fixture::new();
    for api19 in [false, true] {
        for extract in [false, true] {
            let resources = resources(api19);
            let input = fixture.prepare(&resources, extract, api19);
            let mut opts = fixture.options(
                &input,
                &resources,
                &format!("输出中文😀-{extract}-{api19}.apk"),
            );
            if api19 {
                opts.excluded_abis.push("armeabi".into());
            }
            if extract {
                opts.environment_policy = EnvironmentPolicy::Strict;
            }
            let snapshot = assertions::digest(&input);
            let resource_snapshot = assertions::digest(&resources);
            fs::write(&opts.output, "只允许成功写回时替换的旧输出").unwrap();
            let work = fixture.workspace(&format!("protect-{extract}-{api19}"));
            let context = Fixture::context(&work);
            let stages = Arc::new(Mutex::new(Vec::new()));
            let capture = stages.clone();
            protect_apk_with_context(
                &context,
                &fixture.java,
                &opts,
                move |event| capture.lock().unwrap().push(event.step),
                Arc::new(AtomicBool::new(false)),
            )
            .unwrap();
            let mut stages = stages.lock().unwrap().clone();
            stages.dedup();
            assert_eq!(
                stages,
                [
                    ProgressStep::CheckTools,
                    ProgressStep::Unpack,
                    ProgressStep::ModifyManifest,
                    ProgressStep::ProcessDex,
                    ProgressStep::InjectRuntime,
                    ProgressStep::Repack,
                    ProgressStep::AlignApk
                ]
            );
            assertions::verify(&fixture, &opts, extract, api19);
            assert_eq!(assertions::digest(&input), snapshot);
            assert_eq!(assertions::digest(&resources), resource_snapshot);
            assert!(work.is_dir(), "借用材料不得由核心自动删除");
            // Windows 旧 argv 链路的中文缺陷尚未切换；不把已知旧路径当作新适配通过条件。
            if cfg!(not(windows)) && extract && !api19 {
                let mut legacy = opts.clone();
                legacy.output = fixture.root.path().join("legacy.apk");
                protect_apk(&legacy, |_| {}, Arc::new(AtomicBool::new(false))).unwrap();
                // verify 使用独立解密／原库／清单语义检查，不要求随机别名、IKM、nonce 字节相同。
                assertions::verify(&fixture, &legacy, extract, false);
            }
        }
    }
}

#[test]
#[ignore = "需正式适配 JAR、真实壳资源、自有 DEX APK；阶段取消不是运行中硬超时"]
fn 前置拒绝阶段取消及写回失败保留原包和材料() {
    let fixture = Fixture::new();
    let resources = resources(false);
    let input = fixture.prepare(&resources, false, false);
    let snapshot = assertions::digest(&input);
    let base = fixture.options(&input, &resources, "must-not-change.apk");
    let marker = "已有输出不得在失败时被删除".as_bytes();
    fs::write(&base.output, marker).unwrap();
    let context = Fixture::context(&fixture.workspace("negative"));
    let mut wrong_cert = base.clone();
    wrong_cert.expected_output_cert_fingerprint = Some("0".repeat(64));
    assert!(protect_apk_with_context(
        &context,
        &fixture.java,
        &wrong_cert,
        |event| assert_eq!(event.step, ProgressStep::CheckTools),
        Arc::new(AtomicBool::new(false))
    )
    .is_err());
    let mut unsigned = base.clone();
    unsigned.input = PathBuf::from(std::env::var_os("SHIELD_TEST_PROTECT_APK").unwrap());
    assert!(
        !crate::check_apk_with_context(&context, &fixture.java, &unsigned.input, &fixture.signer)
            .unwrap()
            .is_signed
    );
    assert_eq!(
        crate::preflight_apk_with_context(
            &context,
            &fixture.java,
            &unsigned.input,
            crate::PreflightOptions {
                runtime_profile: crate::RuntimeProfile::Standard,
                apksigner_path: Some(&fixture.signer),
                expected_output_cert_fingerprint: None
            }
        )
        .unwrap()
        .verdict,
        crate::PreflightSeverity::Blocked
    );
    let invalid_tool = fixture.root.path().join("invalid.jar");
    fs::write(&invalid_tool, "不是有效工具，不能伪装为无签名").unwrap();
    assert!(
        crate::check_apk_with_context(&context, &fixture.java, &unsigned.input, &invalid_tool)
            .is_err()
    );
    assert!(protect_apk_with_context(
        &context,
        &fixture.java,
        &unsigned,
        |event| assert_eq!(event.step, ProgressStep::CheckTools),
        Arc::new(AtomicBool::new(false))
    )
    .is_err());
    assert!(matches!(
        protect_apk_with_context(
            &context,
            Path::new("missing"),
            &base,
            |_| panic!("不应启动"),
            Arc::new(AtomicBool::new(true))
        ),
        Err(ShieldError::Cancelled)
    ));
    for stage in [
        ProgressStep::CheckTools,
        ProgressStep::Unpack,
        ProgressStep::ModifyManifest,
        ProgressStep::ProcessDex,
        ProgressStep::InjectRuntime,
        ProgressStep::Repack,
        ProgressStep::AlignApk,
    ] {
        let work = fixture.workspace(&format!("cancel-{stage:?}"));
        let context = Fixture::context(&work);
        let cancelled = Arc::new(AtomicBool::new(false));
        let flag = cancelled.clone();
        let result = protect_apk_with_context(
            &context,
            &fixture.java,
            &base,
            move |event| {
                if event.step == stage {
                    flag.store(true, Ordering::Relaxed);
                }
            },
            cancelled,
        );
        assert!(
            matches!(result, Err(ShieldError::Cancelled)),
            "阶段取消应返回取消：{stage:?}"
        );
        assert!(work.is_dir());
        assert_eq!(fs::read(&base.output).unwrap(), marker);
    }
    let mut fail_commit = base.clone();
    fail_commit.output = fixture.workspace("existing-output-directory");
    fs::write(fail_commit.output.join("keep"), marker).unwrap();
    let work = fixture.workspace("commit-failure");
    let context = Fixture::context(&work);
    assert!(protect_apk_with_context(
        &context,
        &fixture.java,
        &fail_commit,
        |_| {},
        Arc::new(AtomicBool::new(false))
    )
    .is_err());
    assert_eq!(fs::read(fail_commit.output.join("keep")).unwrap(), marker);
    assert!(walkdir::WalkDir::new(&work)
        .into_iter()
        .any(|entry| entry.unwrap().file_name() == "candidate.apk"));
    assert_eq!(assertions::digest(&input), snapshot);
    assert_eq!(fs::read(&base.output).unwrap(), marker);
    println!("完整加固负向回归通过：错误证书前置拒绝、七阶段取消、写回失败保留原包／输出／借用材料；不等同整组停止证明");
}

#[test]
fn 显式加固不补选缺失材料且拒绝输入自身作为输出() {
    let root = tempfile::tempdir().unwrap();
    let material = root.path().join("selected");
    fs::write(&material, "只验证启动前路径，不执行夹具").unwrap();
    let opts = ProtectOptions {
        input: material.clone(),
        output: root.path().join("output.apk"),
        apktool_path: Some(material.clone()),
        apksigner_path: Some(material.clone()),
        resources_path: Some(material.clone()),
        expected_output_cert_fingerprint: None,
        environment_policy: EnvironmentPolicy::Compatible,
        excluded_abis: vec![],
    };
    assert!(toolchain::explicit_options(&opts).is_ok());
    for index in 0..3 {
        let mut invalid = opts.clone();
        match index {
            0 => invalid.apktool_path = None,
            1 => invalid.apksigner_path = None,
            _ => invalid.resources_path = None,
        }
        assert!(toolchain::explicit_options(&invalid).is_err());
    }
    let mut same = opts.clone();
    same.output = material.clone();
    assert!(toolchain::explicit_options(&same).is_err());
    #[cfg(unix)]
    {
        let alias = root.path().join("alias.apk");
        std::os::unix::fs::symlink(&material, &alias).unwrap();
        same.output = alias;
        assert!(toolchain::explicit_options(&same).is_err());
    }
    assert!(!opts.output.exists());
}

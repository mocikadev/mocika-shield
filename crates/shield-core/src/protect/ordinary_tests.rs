//! 普通默认工具路径复用同一套独立断言；只使用自有 APK 及合成证书。
use super::{context_assertions as assertions, context_fixture::Fixture, *};
use std::path::{Path, PathBuf};

fn resources(api19: bool) -> PathBuf {
    std::env::var_os(if api19 {
        "SHIELD_TEST_API19_RESOURCES"
    } else {
        "SHIELD_TEST_STANDARD_RESOURCES"
    })
    .expect("指定匹配真实资源")
    .into()
}

fn roots(parent: &Path) -> Vec<PathBuf> {
    fs::read_dir(parent)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| {
            path.file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("java-tool-")
        })
        .collect()
}

fn protect(
    opts: &ProtectOptions,
    progress: impl Fn(ProgressEvent) + Send + 'static,
    cancel: Arc<AtomicBool>,
) -> Result<(), ShieldError> {
    #[cfg(windows)]
    return protect_apk(opts, progress, cancel);
    #[cfg(not(windows))]
    protect_default(opts, progress, cancel)
}

#[test]
#[ignore = "需正式适配资源、双壳资源及自有 DEX APK，实际运行普通默认加固"]
fn 普通加固双资源矩阵保持证书绑定压缩对齐且成功清理() {
    let fixture = Fixture::new();
    for api19 in [false, true] {
        for extract in [false, true] {
            let resources = resources(api19);
            let input = fixture.prepare(&resources, extract, api19);
            let mut opts = fixture.options(
                &input,
                &resources,
                &format!("普通输出中文😀-{api19}-{extract}.apk"),
            );
            if api19 {
                opts.excluded_abis.push("armeabi".into());
            }
            if extract {
                opts.environment_policy = EnvironmentPolicy::Strict;
            }
            let before = fs::read(&input).unwrap();
            fs::write(&opts.output, b"old-output").unwrap();
            protect(&opts, |_| {}, Arc::new(AtomicBool::new(false))).unwrap();
            assert!(
                roots(fixture.root.path()).is_empty(),
                "成功只回收本次普通材料"
            );
            assertions::verify(&fixture, &opts, extract, api19);
            assert_eq!(fs::read(&input).unwrap(), before);
        }
    }
    println!("普通默认加固：标准／工控及压缩四矩阵、原 DEX 恢复、证书绑定、原库保留、16 KB 对齐、输入不变和成功清理均通过；未安装设备");
}

#[test]
#[ignore = "需真实壳及 APK，验证普通调用阶段取消及写回失败材料保留"]
fn 普通阶段取消保留原包输出和材料且不自动回退() {
    let fixture = Fixture::new();
    let resources = resources(false);
    let input = fixture.prepare(&resources, false, false);
    let opts = fixture.options(&input, &resources, "取消输出.apk");
    fs::write(&opts.output, b"existing-output").unwrap();
    let before = fs::read(&input).unwrap();
    let cancelled = Arc::new(AtomicBool::new(false));
    let flag = cancelled.clone();
    assert!(matches!(
        protect(
            &opts,
            move |event| {
                if event.step == ProgressStep::ModifyManifest {
                    flag.store(true, Ordering::Relaxed);
                }
            },
            cancelled
        ),
        Err(ShieldError::Cancelled)
    ));
    assert_eq!(fs::read(&input).unwrap(), before);
    assert_eq!(fs::read(&opts.output).unwrap(), b"existing-output");
    let preserved = roots(fixture.root.path());
    assert_eq!(preserved.len(), 1);
    assert!(walkdir::WalkDir::new(&preserved[0])
        .into_iter()
        .any(|entry| entry.unwrap().file_name() == "decoded"));
    let mut failed = opts.clone();
    failed.output = fixture.root.path().join("既有目录");
    fs::create_dir(&failed.output).unwrap();
    fs::write(failed.output.join("keep"), b"original").unwrap();
    assert!(protect(&failed, |_| {}, Arc::new(AtomicBool::new(false))).is_err());
    assert_eq!(fs::read(failed.output.join("keep")).unwrap(), b"original");
    assert_eq!(roots(fixture.root.path()).len(), 2);
    assert!(walkdir::WalkDir::new(fixture.root.path())
        .into_iter()
        .any(|entry| entry.unwrap().file_name() == "candidate.apk"));
    assert_eq!(fs::read(&input).unwrap(), before);
    println!("普通默认加固失败：阶段取消及写回失败保留原包、既有输出和私有材料，不自动回退；非运行中硬取消证明");
}

#[test]
fn 默认资源选择不伪装自定义资源且开始前取消优先() {
    let root = tempfile::tempdir().unwrap();
    let material = root.path().join("tool.jar");
    fs::write(&material, "仅验证选择规则").unwrap();
    let opts = ProtectOptions {
        excluded_abis: vec![],
        input: root.path().join("missing.apk"),
        output: root.path().join("output.apk"),
        apktool_path: Some(material.clone()),
        apksigner_path: Some(material),
        resources_path: None,
        expected_output_cert_fingerprint: None,
        environment_policy: EnvironmentPolicy::Compatible,
    };
    let selected = toolchain::ordinary_options(&opts).unwrap();
    assert!(selected.resources_path.is_none());
    assert!(selected.apktool_path.unwrap().is_absolute());
    assert!(toolchain::explicit_options(&opts).is_err());
    assert!(matches!(
        protect_default(
            &opts,
            |_| panic!("不得执行"),
            Arc::new(AtomicBool::new(true))
        ),
        Err(ShieldError::Cancelled)
    ));
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 1);
}

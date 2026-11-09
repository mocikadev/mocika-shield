//! 独立签名复用父端唯一交付门槛，相关记录和无关偏好分别处理。
use super::test_fixture as fixture;
use super::*;
use fixture::Fixture;
use shield_app::{
    certificates::{CertificatePreferencesPatch, CertificateStoreState, CertificateUpsertInput},
    paths::ApplicationPaths,
};
use std::fs;

fn draft(name: &str) -> CertificateUpsertInput {
    CertificateUpsertInput {
        id: None,
        name: name.into(),
        source_type: "external".into(),
        keystore_path: String::new(),
        keystore_password: Fixture::password().into(),
        key_alias: "agentfixture".into(),
        key_password: String::new(),
        ks_type: Some("PKCS12".into()),
        sign_v1: true,
        sign_v2: true,
        sign_v3: true,
        sign_v4: false,
        auto_sign_enabled: false,
        note: String::new(),
        set_as_default: true,
        copy_keystore_to_managed: false,
        managed_file_name: None,
    }
}
fn executable() -> PathBuf {
    let program = PathBuf::from(
        std::env::var_os("SHIELD_AGENT_TEST_EXECUTABLE").expect("指定同一特性构建的 CLI"),
    );
    assert!(program.is_absolute() && program.is_file());
    program
}

#[test]
#[ignore = "需要真实 JDK、自有 SHIELD_AGENT_TEST_APK 和显式执行器，逐个验证父端提交检查"]
fn 独立签名默认项备注变化不换证书而材料漂移取消抢占都不交付() {
    for action in [
        "metadata", "default", "material", "input", "cancel", "occupied",
    ] {
        let fixture = Fixture::new();
        let original_config = fs::read(&fixture.config).unwrap();
        let paths = ApplicationPaths::isolated(&fixture.root().join("共享资料")).unwrap();
        let store =
            CertificateStoreState::initialize(paths.certificate_database(), paths.keystore_dir())
                .unwrap();
        let certificate = store
            .save_certificate(
                &draft("原证书"),
                fixture.root().join("临时测试证书.p12").to_str().unwrap(),
                "agentfixture",
                None,
            )
            .unwrap();
        let config = fixture.root().join("仅签名.toml");
        fs::write(
            &config,
            format!(
                "schema_version=1\n[sign]\napksigner={}\n",
                serde_json::to_string(fixture.signer.to_str().unwrap()).unwrap()
            ),
        )
        .unwrap();
        let mut request = Request {
            config: Some(config),
            shared: None,
            sign: Some(crate::automation::sign_plan::Request {
                app_home: Some(fixture.root().join("共享资料")),
                certificate_id: None,
                binding: None,
            }),
            input: fixture.input.clone(),
            output: fixture.output.clone(),
            runtime_profile: Profile::Standard,
            exclude_abis: vec![],
            timeout_seconds: 120,
        };
        request.validate().ok().unwrap();
        let cancel = Arc::new(AtomicBool::new(false));
        let program = executable();
        let mut reached = false;
        let value = run_with_options(
            request,
            cancel.clone(),
            || {
                let mut command = fixture.command(&program);
                command.env_remove("MOCIKA_SHIELD_KS_PASS");
                command
            },
            RunMode::Probe,
            |notice| {
                if !matches!(notice, Notice::Publishing) {
                    return;
                }
                reached = true;
                match action {
                    "metadata" => {
                        store
                            .patch_certificate_preferences(
                                &certificate.id,
                                CertificatePreferencesPatch {
                                    name: Some("新名称".into()),
                                    note: Some("无关备注".into()),
                                    ..Default::default()
                                },
                                false,
                            )
                            .unwrap();
                    }
                    "default" => {
                        store
                            .save_certificate(
                                &draft("不能重选的默认项"),
                                fixture
                                    .root()
                                    .join("不存在的新默认证书.p12")
                                    .to_str()
                                    .unwrap(),
                                "different",
                                None,
                            )
                            .unwrap();
                    }
                    "material" => {
                        store
                            .patch_certificate_preferences(
                                &certificate.id,
                                CertificatePreferencesPatch {
                                    sign_v3: Some(false),
                                    ..Default::default()
                                },
                                false,
                            )
                            .unwrap();
                    }
                    "input" => {
                        use std::io::Write;
                        fs::OpenOptions::new()
                            .append(true)
                            .open(&fixture.input)
                            .unwrap()
                            .write_all(b"changed")
                            .unwrap();
                    }
                    "cancel" => cancel.store(true, Ordering::SeqCst),
                    "occupied" => fs::write(&fixture.output, b"other-task").unwrap(),
                    _ => unreachable!(),
                }
            },
        )
        .unwrap_or_else(|e| panic!("有限签名结果丢失：{}", e.code));
        assert!(reached, "签名必须实际完成并到达父端提交门槛：{value}");
        assert_eq!(value["cleanup_complete"], true);
        assert!(!value.to_string().contains(Fixture::password()));
        assert!(!value.to_string().contains(fixture.root().to_str().unwrap()));
        if matches!(action, "metadata" | "default") {
            assert_eq!(value["status"], "succeeded");
            assert_eq!(value["output_state"], "published");
            assert_eq!(
                shield_core::inspect_signing_input(&fixture.output, || Ok(())).unwrap(),
                shield_core::inspect_signing_input(&fixture.input, || Ok(())).unwrap()
            );
        } else {
            assert_eq!(value["output_state"], "not_published");
            assert_eq!(
                value["status"],
                if action == "cancel" {
                    "cancelled"
                } else {
                    "failed"
                }
            );
            if matches!(action, "material" | "input") {
                assert_eq!(value["code"], "PLAN_SOURCE_CHANGED");
            }
            if action == "occupied" {
                assert_eq!(fs::read(&fixture.output).unwrap(), b"other-task");
            } else {
                assert!(!fixture.output.exists());
            }
        }
        if action != "input" {
            fixture.assert_inputs_unchanged();
        }
        fixture.assert_no_tasks();
        assert_eq!(fs::read(&fixture.config).unwrap(), original_config);
    }
}

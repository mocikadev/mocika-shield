use super::*;
use crate::automation::snapshot;
#[cfg(unix)]
use std::os::unix::fs::{symlink, PermissionsExt};
#[cfg(windows)]
use std::os::windows::fs::symlink_file as symlink;

fn make_public(path: &Path, directory: bool) {
    #[cfg(unix)]
    fs::set_permissions(
        path,
        fs::Permissions::from_mode(if directory { 0o755 } else { 0o644 }),
    )
    .unwrap();
    #[cfg(windows)]
    task_files::test_make_public(path, directory);
}

struct Fixture {
    root: tempfile::TempDir,
    sources: Vec<(Role, FileSnapshot)>,
    work: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        #[cfg(unix)]
        fs::set_permissions(root.path(), fs::Permissions::from_mode(0o700)).unwrap();
        let work = root.path().join("私有暂存 空格");
        task_files::private_directory(&work).unwrap();
        let sources = ROLES
            .into_iter()
            .map(|role| {
                let path = root.path().join(format!("原始中文-{}", role.name()));
                fs::write(&path, format!("{}：仅为测试构造的材料", role.name())).unwrap();
                (role, snapshot::file(role.name(), &path).ok().unwrap())
            })
            .collect();
        Self {
            root,
            sources,
            work,
        }
    }
    fn refs(&self) -> Vec<(Role, &FileSnapshot)> {
        self.sources
            .iter()
            .map(|(role, source)| (*role, source))
            .collect()
    }
    fn create(&self) -> MaterialBundle {
        MaterialBundle::create(&self.work, &self.refs(), || Ok(())).unwrap()
    }
}

#[test]
fn 六类材料固化且索引不包含路径和凭据() {
    let fixture = Fixture::new();
    let bundle = fixture.create();
    #[cfg(unix)]
    assert_eq!(
        fs::metadata(&bundle.root).unwrap().permissions().mode() & 0o777,
        0o700
    );
    for (role, source) in &fixture.sources {
        assert_eq!(
            fs::read(bundle.path(*role)).unwrap(),
            fs::read(&source.path).unwrap()
        );
        #[cfg(unix)]
        assert_eq!(
            fs::metadata(bundle.path(*role))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        task_files::validate_private_file(&File::open(bundle.path(*role)).unwrap()).unwrap();
    }
    let index = fs::read_to_string(bundle.root.join("materials.json")).unwrap();
    assert!(!index.contains(fixture.root.path().to_str().unwrap()));
    for word in ["password", "key_alias", "lookup_path"] {
        assert!(!index.contains(word));
    }
    MaterialBundle::verify(&bundle.root, bundle.digest(), || Ok(())).unwrap();
    fs::write(&fixture.sources[0].1.path, b"source later changed").unwrap();
    MaterialBundle::verify(&bundle.root, bundle.digest(), || Ok(())).unwrap();
    assert_ne!(
        fs::read(bundle.path(Role::Input)).unwrap(),
        b"source later changed"
    );
}

#[test]
fn 新适配材料独立版本不可冒充旧集合且拒绝缺项和额外文件() {
    for version in [4, 5] {
        for config in [false, true] {
            for defect in 0..3 {
                let fixture = Fixture::new();
                let helper = fixture.root.path().join("helper.jar");
                fs::write(&helper, "只验证固定角色，不执行此夹具").unwrap();
                let snapshot = snapshot::file("java_adapter", &helper).ok().unwrap();
                let mut refs = fixture
                    .refs()
                    .into_iter()
                    .filter(|(role, _)| {
                        (config || *role != Role::Config)
                            && (version == 4 || !matches!(role, Role::Apktool | Role::Resources))
                    })
                    .collect::<Vec<_>>();
                refs.push((Role::JavaAdapter, &snapshot));
                assert!(MaterialBundle::create_version(
                    &fixture.work,
                    &refs,
                    if version == 4 { 2 } else { 3 },
                    || Ok(())
                )
                .is_err());
                let bundle =
                    MaterialBundle::create_version(&fixture.work, &refs, version, || Ok(()))
                        .unwrap();
                MaterialBundle::verify_sources_version(&bundle.root, &refs, version, || Ok(()))
                    .unwrap();
                assert!(MaterialBundle::verify_sources_version(
                    &bundle.root,
                    &refs,
                    if version == 4 { 2 } else { 3 },
                    || Ok(())
                )
                .is_err());
                match defect {
                    0 => {
                        fs::remove_file(bundle.path(Role::JavaAdapter)).unwrap();
                    }
                    1 => {
                        fs::write(bundle.root.join("unexpected.jar"), b"extra").unwrap();
                    }
                    _ => {
                        refs.pop();
                    }
                }
                assert!(MaterialBundle::verify_sources_version(
                    &bundle.root,
                    &refs,
                    version,
                    || Ok(())
                )
                .is_err());
            }
        }
    }
}

#[test]
fn 共享无项目使用新版五角色且不能冒充旧版缺项() {
    let fixture = Fixture::new();
    let refs: Vec<_> = fixture
        .refs()
        .into_iter()
        .filter(|(role, _)| *role != Role::Config)
        .collect();
    assert!(matches!(
        MaterialBundle::create(&fixture.work, &refs, || Ok(())),
        Err(MaterialError::InvalidSet)
    ));
    let bundle = MaterialBundle::create_shared(&fixture.work, &refs, || Ok(())).unwrap();
    assert!(!bundle.path(Role::Config).exists());
    let index = fs::read(bundle.root.join("materials.json")).unwrap();
    let mut value: serde_json::Value = serde_json::from_slice(&index).unwrap();
    assert_eq!(value["format_version"], 2);
    assert_eq!(value["entries"].as_array().unwrap().len(), 5);
    MaterialBundle::verify_shared_sources(&bundle.root, &refs, || Ok(())).unwrap();
    value["format_version"] = serde_json::json!(1);
    let bytes = serde_json::to_vec(&value).unwrap();
    fs::write(bundle.root.join("materials.json"), &bytes).unwrap();
    assert!(matches!(
        MaterialBundle::verify(&bundle.root, Sha256::digest(bytes).into(), || Ok(())),
        Err(MaterialError::InvalidBundle)
    ));
}

#[test]
fn 共享有项目仍固定六角色并拒绝重复缺项及未知版本() {
    let fixture = Fixture::new();
    let refs = fixture.refs();
    let bundle = MaterialBundle::create_shared(&fixture.work, &refs, || Ok(())).unwrap();
    MaterialBundle::verify_shared_sources(&bundle.root, &refs, || Ok(())).unwrap();
    let mut duplicated = refs.clone();
    duplicated[1] = duplicated[0];
    assert!(MaterialBundle::verify_shared_sources(&bundle.root, &duplicated, || Ok(())).is_err());
    assert!(MaterialBundle::verify_shared_sources(&bundle.root, &refs[..5], || Ok(())).is_err());
    let mut manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(bundle.root.join("materials.json")).unwrap()).unwrap();
    manifest["format_version"] = serde_json::json!(3);
    let bytes = serde_json::to_vec(&manifest).unwrap();
    fs::write(bundle.root.join("materials.json"), &bytes).unwrap();
    assert!(MaterialBundle::verify(&bundle.root, Sha256::digest(bytes).into(), || Ok(())).is_err());
}

#[test]
fn 独立签名仅接受三类材料及可选项目且不能借此放宽加固() {
    for has_config in [false, true] {
        let fixture = Fixture::new();
        let refs: Vec<_> = fixture
            .refs()
            .into_iter()
            .filter(|(role, _)| {
                matches!(role, Role::Input | Role::Keystore | Role::Apksigner)
                    || (has_config && *role == Role::Config)
            })
            .collect();
        assert!(MaterialBundle::create(&fixture.work, &refs, || Ok(())).is_err());
        assert!(MaterialBundle::create_shared(&fixture.work, &refs, || Ok(())).is_err());
        let bundle = MaterialBundle::create_version(&fixture.work, &refs, 3, || Ok(())).unwrap();
        assert!(!bundle.path(Role::Apktool).exists());
        assert!(!bundle.path(Role::Resources).exists());
        MaterialBundle::verify_sources_version(&bundle.root, &refs, 3, || Ok(())).unwrap();
        assert!(MaterialBundle::verify_shared_sources(&bundle.root, &refs, || Ok(())).is_err());
        for invalid in [
            refs[..refs.len() - 1].to_vec(),
            {
                let mut v = refs.clone();
                v.push(refs[0]);
                v
            },
            fixture.refs(),
        ] {
            assert!(
                MaterialBundle::verify_sources_version(&bundle.root, &invalid, 3, || Ok(()))
                    .is_err()
            );
        }
        let mut manifest: serde_json::Value =
            serde_json::from_slice(&fs::read(bundle.root.join("materials.json")).unwrap()).unwrap();
        for version in [1, 2, 4] {
            manifest["format_version"] = serde_json::json!(version);
            let bytes = serde_json::to_vec(&manifest).unwrap();
            fs::write(bundle.root.join("materials.json"), &bytes).unwrap();
            assert!(
                MaterialBundle::verify(&bundle.root, Sha256::digest(bytes).into(), || Ok(()))
                    .is_err()
            );
        }
    }
}

#[test]
fn 材料不完整重复或角色错配均拒绝() {
    let fixture = Fixture::new();
    let refs = fixture.refs();
    assert!(matches!(
        MaterialBundle::create(&fixture.work, &refs[..5], || Ok(())),
        Err(MaterialError::InvalidSet)
    ));
    let mut duplicated = refs.clone();
    duplicated[0] = duplicated[1];
    assert!(matches!(
        MaterialBundle::create(&fixture.work, &duplicated, || Ok(())),
        Err(MaterialError::InvalidSet)
    ));
    let mut swapped = refs;
    swapped.swap(0, 1);
    swapped[0].0 = Role::Input;
    swapped[1].0 = Role::Config;
    assert!(matches!(
        MaterialBundle::create(&fixture.work, &swapped, || Ok(())),
        Err(MaterialError::InvalidSet)
    ));
}

#[test]
fn 材料变更失败且不会生成完整索引() {
    let fixture = Fixture::new();
    fs::write(&fixture.sources[0].1.path, b"changed").unwrap();
    assert!(matches!(
        MaterialBundle::create(&fixture.work, &fixture.refs(), || Ok(())),
        Err(MaterialError::SourceChanged)
    ));
    assert!(!fixture.work.join("materials/materials.json").exists());
    assert_eq!(fs::read(&fixture.sources[0].1.path).unwrap(), b"changed");
}

#[test]
fn 原始材料链接改向即使字节相同也拒绝() {
    let mut fixture = Fixture::new();
    let source = &mut fixture.sources[0].1;
    let link = fixture.root.path().join("选择的输入");
    symlink(&source.path, &link).unwrap();
    *source = snapshot::file("input", &link).ok().unwrap();
    let other = fixture.root.path().join("同内容不同源");
    fs::write(&other, fs::read(&source.path).unwrap()).unwrap();
    fs::remove_file(&link).unwrap();
    symlink(other, &link).unwrap();
    assert!(matches!(
        MaterialBundle::create(&fixture.work, &fixture.refs(), || Ok(())),
        Err(MaterialError::SourceChanged)
    ));
}

#[test]
fn 不覆盖既有材料目录或非私有目标() {
    let fixture = Fixture::new();
    fs::create_dir(fixture.work.join("materials")).unwrap();
    let marker = fixture.work.join("materials/已有文件");
    fs::write(&marker, b"keep").unwrap();
    assert!(matches!(
        MaterialBundle::create(&fixture.work, &fixture.refs(), || Ok(())),
        Err(MaterialError::Io(io::ErrorKind::AlreadyExists))
    ));
    assert_eq!(fs::read(marker).unwrap(), b"keep");
    make_public(&fixture.work, true);
    assert!(matches!(
        MaterialBundle::create(&fixture.work, &fixture.refs(), || Ok(())),
        Err(MaterialError::InvalidBundle)
    ));
}

#[test]
fn 复制取消不修改源材料且没有完成索引() {
    let fixture = Fixture::new();
    let original = fs::read(&fixture.sources[0].1.path).unwrap();
    let mut points = 0;
    let result = MaterialBundle::create(&fixture.work, &fixture.refs(), || {
        points += 1;
        if points == 3 {
            Err(MaterialError::Interrupted)
        } else {
            Ok(())
        }
    });
    assert!(matches!(result, Err(MaterialError::Interrupted)));
    assert!(!fixture.work.join("materials/materials.json").exists());
    assert_eq!(fs::read(&fixture.sources[0].1.path).unwrap(), original);
}

#[test]
fn 索引摘要材料篡改及链接均无法通过重新读取() {
    for kind in 0..5 {
        let fixture = Fixture::new();
        let bundle = fixture.create();
        let file = bundle.path(Role::Input);
        match kind {
            0 => {
                assert!(MaterialBundle::verify(&bundle.root, [0; 32], || Ok(())).is_err());
                continue;
            }
            1 => fs::write(&file, b"corrupted").unwrap(),
            2 => {
                fs::remove_file(&file).unwrap();
                symlink(&fixture.sources[0].1.path, &file).unwrap();
            }
            3 => {
                fs::remove_file(&file).unwrap();
                fs::hard_link(&fixture.sources[0].1.path, &file).unwrap();
            }
            _ => make_public(&file, false),
        }
        assert!(MaterialBundle::verify(&bundle.root, bundle.digest(), || Ok(())).is_err());
    }
}

#[test]
fn 未知字段重复角色超限索引即使摘要匹配也拒绝() {
    for kind in 0..4 {
        let fixture = Fixture::new();
        let bundle = fixture.create();
        let path = bundle.root.join("materials.json");
        let mut value: serde_json::Value =
            serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        let bytes = match kind {
            0 => {
                value["path"] = serde_json::json!("不可选择任意路径");
                serde_json::to_vec(&value).unwrap()
            }
            1 => {
                value["entries"][0] = value["entries"][1].clone();
                serde_json::to_vec(&value).unwrap()
            }
            2 => {
                value["format_version"] = serde_json::json!(3);
                serde_json::to_vec(&value).unwrap()
            }
            _ => vec![b' '; 4097],
        };
        fs::write(path, &bytes).unwrap();
        assert!(matches!(
            MaterialBundle::verify(&bundle.root, Sha256::digest(bytes).into(), || Ok(())),
            Err(MaterialError::InvalidBundle)
        ));
    }
}

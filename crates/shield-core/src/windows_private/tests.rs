use super::*;
#[allow(dead_code)]
mod permissions {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/fixtures/windows_permissions.rs"
    ));
}

#[test]
fn 私有创建具有受保护访问控制且不覆盖已有目录或文件() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("private");
    create_directory(&path).unwrap();
    let directory = crate::java_directory::open_directory(&path).unwrap();
    validate(&directory, true).unwrap();
    assert!(create_directory(&path).is_err());
    let path = path.join("request.bin");
    let mut file = create_file(&path).unwrap();
    use std::io::Write;
    file.write_all(b"private").unwrap();
    validate(&file, false).unwrap();
    drop(file);
    assert!(create_file(&path).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), b"private");
    let stream = path.with_file_name("request.bin:extra");
    assert!(create_file(&stream).is_err());
    assert!(std::fs::read(&stream).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), b"private");
}

#[test]
fn 目录或文件权限放宽和空访问列表均拒绝且不自动修复() {
    for directory in [false, true] {
        for acl in ["D:P(A;;FA;;;WD)", "D:NO_ACCESS_CONTROL"] {
            let root = tempfile::tempdir().unwrap();
            let path = root.path().join("private");
            if directory {
                create_directory(&path).unwrap();
            } else {
                drop(create_file(&path).unwrap());
            }
            let control = permissions::control(&path, directory);
            permissions::set_acl(&control, acl);
            assert!(validate(&control, directory).is_err());
            assert!(
                validate(&control, directory).is_err(),
                "校验不能修复用户权限"
            );
        }
    }
}

#[test]
fn 无读取权限文件不会降权或修改权限重试() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("private.bin");
    drop(create_file(&path).unwrap());
    let control = permissions::control(&path, false);
    permissions::set_acl(&control, "D:P");
    assert!(readonly(&path).is_err());
    assert!(validate(&control, false).is_err());
    // 仅恢复本测试文件的清理能力，正式接口没有修复权限行为。
    permissions::make_public(&control);
}

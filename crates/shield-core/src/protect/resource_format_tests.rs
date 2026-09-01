use super::resource_format::normalize_mislabeled_jpeg_resources;
use std::fs;

const JPEG_PREFIX: &[u8] = &[0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10];

#[test]
fn jpeg_伪装_png_会无损改名为_jpg() {
    let dir = tempfile::tempdir().unwrap();
    let resource = dir.path().join("res/mipmap-xhdpi/ic_bottom_sucai.png");
    fs::create_dir_all(resource.parent().unwrap()).unwrap();
    fs::write(&resource, JPEG_PREFIX).unwrap();

    let renamed = normalize_mislabeled_jpeg_resources(dir.path()).unwrap();

    let target = resource.with_extension("jpg");
    assert_eq!(renamed, 1);
    assert!(!resource.exists());
    assert_eq!(fs::read(target).unwrap(), JPEG_PREFIX);
}

#[test]
fn 九宫格_png_伪装_jpeg_会被拒绝而不改名() {
    let dir = tempfile::tempdir().unwrap();
    let resource = dir.path().join("res/drawable/button.9.png");
    fs::create_dir_all(resource.parent().unwrap()).unwrap();
    fs::write(&resource, JPEG_PREFIX).unwrap();

    let error = normalize_mislabeled_jpeg_resources(dir.path()).unwrap_err();

    assert!(error.to_string().contains("九宫格"));
    assert!(resource.exists());
}

#[test]
fn 已存在同名_jpg_时拒绝覆盖() {
    let dir = tempfile::tempdir().unwrap();
    let resource = dir.path().join("res/mipmap-hdpi/icon.png");
    let target = resource.with_extension("jpg");
    fs::create_dir_all(resource.parent().unwrap()).unwrap();
    fs::write(&resource, JPEG_PREFIX).unwrap();
    fs::write(&target, b"existing").unwrap();

    let error = normalize_mislabeled_jpeg_resources(dir.path()).unwrap_err();

    assert!(error.to_string().contains("同名 .jpg"));
    assert!(resource.exists());
    assert_eq!(fs::read(target).unwrap(), b"existing");
}

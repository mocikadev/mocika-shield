//! 复用资格边界回归；异常元数据应回退，不能跳过普通文件内容校验。

use super::*;
use tempfile::tempdir;

fn word(bytes: &[u8], offset: usize) -> usize {
    u16::from_le_bytes(bytes[offset..offset + 2].try_into().unwrap()) as usize
}

fn dword(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
}

fn make_fixture(path: &Path, directory: bool, method: CompressionMethod) {
    let mut writer = ZipWriter::new(fs::File::create(path).unwrap());
    if directory {
        writer
            .add_directory("assets/", SimpleFileOptions::default())
            .unwrap();
    }
    let mut options = zip::write::FullFileOptions::default()
        .compression_method(method)
        .with_alignment(DEFAULT_ALIGNMENT);
    options
        .add_extra_data(0xCAFE, vec![0; 4].into_boxed_slice(), false)
        .unwrap();
    options
        .add_extra_data(0xBEEF, Vec::new().into_boxed_slice(), true)
        .unwrap();
    writer.start_file("assets/payload.bin", options).unwrap();
    writer.write_all(&[0x5A; 1024]).unwrap();
    writer.finish().unwrap();
    assert!(can_reuse_aligned(path, NativeLibraryPackaging::Preserve).unwrap());
}

fn central_start(path: &Path) -> usize {
    ZipArchive::new(fs::File::open(path).unwrap())
        .unwrap()
        .central_directory_start() as usize
}

fn assert_rewritten(path: &Path, before: &[u8]) {
    assert!(!can_reuse_aligned(path, NativeLibraryPackaging::Preserve).unwrap());
    align_apk(path).unwrap();
    assert!(fs::read(path).unwrap() != before);
    assert!(verify_apk_alignment(path).unwrap().is_empty());
    assert!(can_reuse_aligned(path, NativeLibraryPackaging::Preserve).unwrap());
}

#[test]
fn 附加字段越界或残缺头不得原样复用() {
    for central_field in [false, true] {
        for field_size in [2u16, u16::MAX] {
            let dir = tempdir().unwrap();
            let path = dir.path().join("extra.apk");
            make_fixture(&path, false, CompressionMethod::Stored);
            let central = central_start(&path);
            let mut bytes = fs::read(&path).unwrap();
            let extra = if central_field {
                central + 46 + word(&bytes, central + 28)
            } else {
                30 + word(&bytes, 26)
            };
            bytes[extra + 2..extra + 4].copy_from_slice(&field_size.to_le_bytes());
            fs::write(&path, &bytes).unwrap();
            assert_rewritten(&path, &bytes);
        }
    }
}

#[test]
fn 本地_zip64附加字段即使版本号未升级也不复用() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("local-zip64.apk");
    make_fixture(&path, false, CompressionMethod::Stored);
    let mut bytes = fs::read(&path).unwrap();
    let extra = 30 + word(&bytes, 26);
    bytes[extra..extra + 2].copy_from_slice(&1u16.to_le_bytes());
    fs::write(&path, &bytes).unwrap();
    assert_rewritten(&path, &bytes);
}

#[test]
fn 空目录非零_crc或压缩方法不得原样复用() {
    for bad_crc in [true, false] {
        let dir = tempdir().unwrap();
        let path = dir.path().join("directory.apk");
        make_fixture(&path, true, CompressionMethod::Stored);
        let central = central_start(&path);
        let mut bytes = fs::read(&path).unwrap();
        if bad_crc {
            bytes[14..18].copy_from_slice(&1u32.to_le_bytes());
            bytes[central + 16..central + 20].copy_from_slice(&1u32.to_le_bytes());
        } else {
            bytes[8..10].copy_from_slice(&8u16.to_le_bytes());
            bytes[central + 10..central + 12].copy_from_slice(&8u16.to_le_bytes());
        }
        fs::write(&path, &bytes).unwrap();
        assert_rewritten(&path, &bytes);
        let mut archive = ZipArchive::new(fs::File::open(&path).unwrap()).unwrap();
        let mut entry = archive.by_index(0).unwrap();
        assert_eq!(entry.compression(), CompressionMethod::Stored);
        assert_eq!(entry.crc32(), 0);
        assert_eq!(io::copy(&mut entry, &mut io::sink()).unwrap(), 0);
    }
}

#[test]
fn 条目起始磁盘号非零不得原样复用() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("disk.apk");
    make_fixture(&path, false, CompressionMethod::Stored);
    let central = central_start(&path);
    let mut bytes = fs::read(&path).unwrap();
    bytes[central + 34..central + 36].copy_from_slice(&1u16.to_le_bytes());
    fs::write(&path, &bytes).unwrap();
    assert_rewritten(&path, &bytes);
}

#[test]
fn 合法空目录和不同附加字段仍能原样复用() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("valid.apk");
    make_fixture(&path, true, CompressionMethod::Deflated);
    let before = fs::read(&path).unwrap();
    align_apk(&path).unwrap();
    assert_eq!(fs::read(&path).unwrap(), before);
}

#[test]
fn 元数据回退不能掩盖普通文件_crc错误() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("corrupt.apk");
    make_fixture(&path, false, CompressionMethod::Stored);
    let mut archive = ZipArchive::new(fs::File::open(&path).unwrap()).unwrap();
    let data = archive.by_index(0).unwrap().data_start() as usize;
    drop(archive);
    let mut bytes = fs::read(&path).unwrap();
    let extra = 30 + word(&bytes, 26);
    bytes[extra + 2..extra + 4].copy_from_slice(&u16::MAX.to_le_bytes());
    bytes[data] ^= 1;
    fs::write(&path, &bytes).unwrap();
    assert!(!can_reuse_aligned(&path, NativeLibraryPackaging::Preserve).unwrap());
    assert!(align_apk(&path).is_err());
    assert_eq!(fs::read(&path).unwrap(), bytes);
}

#[test]
fn 解压内容校验不承诺清理压缩流尾随字节() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("tail.apk");
    make_fixture(&path, false, CompressionMethod::Deflated);
    let central = central_start(&path);
    let mut bytes = fs::read(&path).unwrap();
    let end = bytes.len() - 22;
    let compressed_size = dword(&bytes, 18) + 4;
    bytes[18..22].copy_from_slice(&compressed_size.to_le_bytes());
    bytes[central + 20..central + 24].copy_from_slice(&compressed_size.to_le_bytes());
    bytes[end + 16..end + 20].copy_from_slice(&((central + 4) as u32).to_le_bytes());
    bytes.splice(central..central, [0xDE, 0xAD, 0xBE, 0xEF]);
    fs::write(&path, &bytes).unwrap();
    assert!(can_reuse_aligned(&path, NativeLibraryPackaging::Preserve).unwrap());
    align_apk(&path).unwrap();
    assert_eq!(fs::read(&path).unwrap(), bytes);
}

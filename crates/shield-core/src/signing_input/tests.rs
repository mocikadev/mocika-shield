use super::*;
use std::{fs, io::Write, path::PathBuf};
use zip::{write::SimpleFileOptions, CompressionMethod, ZipWriter};

fn archive(root: &Path, name: &str, files: &[(&str, &[u8])], compressed: bool) -> PathBuf {
    let path = root.join(name);
    let mut zip = ZipWriter::new(File::create(&path).unwrap());
    let options = SimpleFileOptions::default().compression_method(if compressed {
        CompressionMethod::Deflated
    } else {
        CompressionMethod::Stored
    });
    for &(name, bytes) in files {
        zip.start_file(name, options).unwrap();
        zip.write_all(bytes).unwrap();
    }
    zip.finish().unwrap();
    path
}
fn inspect(path: &Path) -> Result<SigningInput> {
    inspect_signing_input(path, || Ok(()))
}
fn payload() -> Vec<u8> {
    let mut data = b"DEXB".to_vec();
    data.extend_from_slice(&5u32.to_le_bytes());
    data.extend_from_slice(&1u32.to_le_bytes());
    data.push(64);
    data.extend_from_slice("AB".repeat(32).as_bytes());
    data.push(32);
    data.extend_from_slice(&[1; 32]);
    data.extend_from_slice(&[2; 12]);
    // 这里只构造格式声明，不伪称已通过密文认证或解密。
    data.extend_from_slice(&[3; 16]);
    data
}
fn dex(payload: &[u8], prefix: usize) -> Vec<u8> {
    let mut data = vec![0x30; prefix];
    data.extend_from_slice(b"MSHD");
    data.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    data.extend_from_slice(payload);
    data
}
fn protected(root: &Path, payload: &[u8], prefix: usize) -> PathBuf {
    archive(
        root,
        "加固 中文.apk",
        &[
            ("AndroidManifest.xml", b"manifest"),
            ("classes.dex", &dex(payload, prefix)),
        ],
        true,
    )
}

#[test]
fn 摘要忽略签名元数据顺序及压缩但不忽略业务元数据() {
    let temp = tempfile::tempdir().unwrap();
    let base = [
        ("AndroidManifest.xml", b"manifest".as_slice()),
        ("classes.dex", b"ordinary"),
        ("META-INF/业务配置", b"config"),
    ];
    let input = archive(temp.path(), "input.apk", &base, false);
    let mut changed = base.to_vec();
    changed.reverse();
    changed.extend([
        ("META-INF/MANIFEST.MF", b"signature".as_slice()),
        ("META-INF/CERT.SF", b"sig"),
        ("META-INF/CERT.RSA", b"certificate"),
        ("META-INF/SIG-X", b"other"),
    ]);
    let signed = archive(temp.path(), "signed.apk", &changed, true);
    assert_ne!(fs::read(&input).unwrap(), fs::read(&signed).unwrap());
    assert_eq!(inspect(&input).unwrap(), inspect(&signed).unwrap());
    changed[0] = ("META-INF/业务配置", b"modified");
    let different = archive(temp.path(), "different.apk", &changed, true);
    assert_ne!(inspect(&input).unwrap(), inspect(&different).unwrap());
}

#[test]
fn 清单与业务文件的增删改名均改变摘要() {
    let temp = tempfile::tempdir().unwrap();
    let input = archive(
        temp.path(),
        "input.apk",
        &[
            ("AndroidManifest.xml", b"manifest"),
            ("assets/data", b"original"),
        ],
        false,
    );
    for files in [
        vec![
            ("AndroidManifest.xml", b"changed".as_slice()),
            ("assets/data", b"original"),
        ],
        vec![("AndroidManifest.xml", b"manifest".as_slice())],
        vec![
            ("AndroidManifest.xml", b"manifest".as_slice()),
            ("assets/renamed", b"original"),
        ],
        vec![
            ("AndroidManifest.xml", b"manifest".as_slice()),
            ("assets/data", b"changed"),
        ],
    ] {
        let other = archive(temp.path(), "other.apk", &files, false);
        assert_ne!(inspect(&input).unwrap(), inspect(&other).unwrap());
    }
}

#[test]
fn 当前绑定可读取且头跨分块与大载荷不丢失() {
    let temp = tempfile::tempdir().unwrap();
    for prefix in [1, 64 * 1024 - 3, 64 * 1024 - 9] {
        let mut data = payload();
        data.extend_from_slice(&[5; 128 * 1024]);
        let apk = protected(temp.path(), &data, prefix);
        let before = fs::read(&apk).unwrap();
        assert_eq!(inspect(&apk).unwrap().certificate_binding, Some([0xab; 32]));
        assert_eq!(before, fs::read(&apk).unwrap());
    }
}

#[test]
fn 未加固的魔数文本不冒充绑定() {
    let temp = tempfile::tempdir().unwrap();
    let apk = archive(
        temp.path(),
        "normal.apk",
        &[
            ("AndroidManifest.xml", b"manifest"),
            ("classes.dex", b"ordinary MSHD string"),
        ],
        false,
    );
    assert_eq!(inspect(&apk).unwrap().certificate_binding, None);
}

#[test]
fn 已检测到的未知版本损坏或空绑定不能降为普通包() {
    let temp = tempfile::tempdir().unwrap();
    let original = payload();
    let mut variants = Vec::new();
    for (offset, bytes) in [(4usize, 4u32.to_le_bytes()), (8usize, 0u32.to_le_bytes())] {
        let mut value = original.clone();
        value[offset..offset + 4].copy_from_slice(&bytes);
        variants.push(value);
    }
    for (offset, byte) in [(0, b'X'), (12, 63), (13, b'G'), (77, 0)] {
        let mut value = original.clone();
        value[offset] = byte;
        variants.push(value);
    }
    variants.push(original[..70].to_vec());
    variants.push(original[..original.len() - 1].to_vec());
    for value in variants {
        assert!(inspect(&protected(temp.path(), &value, 128)).is_err());
    }
}

#[test]
fn 长度损坏的连续格式声明与冲突尾块明确拒绝() {
    let temp = tempfile::tempdir().unwrap();
    let mut broken = dex(&payload(), 64 * 1024 - 9);
    broken.push(0);
    let apk = archive(
        temp.path(),
        "broken.apk",
        &[
            ("AndroidManifest.xml", b"manifest"),
            ("classes.dex", &broken),
        ],
        false,
    );
    assert!(inspect(&apk).is_err());
    let mut outer = payload();
    outer.extend_from_slice(&dex(&payload(), 10));
    assert!(inspect(&protected(temp.path(), &outer, 128)).is_err());
}

#[test]
fn 中央目录被库折叠的重复条目仍拒绝() {
    let temp = tempfile::tempdir().unwrap();
    let apk = archive(
        temp.path(),
        "duplicate.apk",
        &[
            ("AndroidManifest.xml", b"manifest"),
            ("assets/one", b"a"),
            ("assets/two", b"b"),
        ],
        false,
    );
    let mut bytes = fs::read(&apk).unwrap();
    for index in 0..=bytes.len() - 10 {
        if &bytes[index..index + 10] == b"assets/two" {
            bytes[index..index + 10].copy_from_slice(b"assets/one");
        }
    }
    fs::write(&apk, bytes).unwrap();
    assert!(inspect(&apk).is_err());
}

#[test]
fn 读取期间可取消且不会写入或展开输入() {
    let temp = tempfile::tempdir().unwrap();
    let apk = archive(
        temp.path(),
        "input.apk",
        &[
            ("AndroidManifest.xml", b"manifest"),
            ("assets/large", &vec![8; 3 * 1024 * 1024]),
        ],
        true,
    );
    let before = fs::read(&apk).unwrap();
    let mut calls = 0;
    let result = inspect_signing_input(&apk, || {
        calls += 1;
        if calls > 10 {
            bail!("测试取消");
        }
        Ok(())
    });
    assert!(result.unwrap_err().to_string().contains("测试取消"));
    assert_eq!(fs::read(&apk).unwrap(), before);
    assert_eq!(fs::read_dir(temp.path()).unwrap().count(), 1);
}

#[test]
fn 缺少清单或业务条目校验损坏拒绝() {
    let temp = tempfile::tempdir().unwrap();
    let apk = archive(
        temp.path(),
        "no-manifest.apk",
        &[("assets/data", b"data")],
        false,
    );
    assert!(inspect(&apk).is_err());
    let apk = archive(
        temp.path(),
        "bad-crc.apk",
        &[("AndroidManifest.xml", b"manifest")],
        false,
    );
    let offset = zip::ZipArchive::new(File::open(&apk).unwrap())
        .unwrap()
        .by_name("AndroidManifest.xml")
        .unwrap()
        .data_start() as usize;
    let mut bytes = fs::read(&apk).unwrap();
    bytes[offset] ^= 1;
    fs::write(&apk, bytes).unwrap();
    assert!(inspect(&apk).is_err());
}

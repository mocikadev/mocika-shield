//! 显式指定本机已对齐样本后运行；只操作临时副本，不改原始 APK。

use sha2::{Digest, Sha256};
use std::io::Read;
use std::path::Path;

fn digest(path: &Path) -> [u8; 32] {
    let mut file = std::fs::File::open(path).unwrap();
    let mut hash = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let count = file.read(&mut buffer).unwrap();
        if count == 0 {
            break;
        }
        hash.update(&buffer[..count]);
    }
    hash.finalize().into()
}

#[test]
#[ignore = "需要 SHIELD_TEST_ALIGNED_APK 指定本机已对齐样本，不纳入日常重型测试"]
fn 真实已对齐样本完整校验后不改字节() {
    let source = std::path::PathBuf::from(std::env::var_os("SHIELD_TEST_ALIGNED_APK").unwrap());
    let before = digest(&source);
    let temp = tempfile::tempdir().unwrap();
    let copy = temp.path().join("aligned.apk");
    std::fs::copy(&source, &copy).unwrap();
    assert!(shield_core::verify_apk_alignment(&copy).unwrap().is_empty());
    let started = std::time::Instant::now();
    shield_core::align_apk(&copy).unwrap();
    println!(
        "已对齐样本校验复用耗时：{} 毫秒",
        started.elapsed().as_millis()
    );
    assert_eq!(digest(&copy), before);
    assert_eq!(digest(&source), before);
}

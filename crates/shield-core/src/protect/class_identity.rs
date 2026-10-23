//! 在业务 DEX 加密前拒绝与壳的完整类名冲突，不尝试重写用户代码。

use anyhow::{bail, Context, Result};
use std::fs;
use std::io::Read;
use std::path::Path;

use crate::dex_classes::defined_classes;

pub(crate) fn validate_class_identity(apk_dir: &Path, resources: &Path) -> Result<()> {
    let mut archive = zip::ZipArchive::new(fs::File::open(resources)?)?;
    let mut stub = Vec::new();
    archive
        .by_name("stub-classes.dex")?
        .read_to_end(&mut stub)?;
    let reserved = defined_classes(&stub).context("壳 DEX 类定义无效")?;
    if reserved.is_empty() {
        bail!("壳 DEX 没有可用类定义");
    }
    let mut paths = fs::read_dir(apk_dir)?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<std::io::Result<Vec<_>>>()?;
    paths.sort();
    for path in paths {
        if !path.is_file() || path.extension().and_then(|value| value.to_str()) != Some("dex") {
            continue;
        }
        let data = fs::read(&path)?;
        let classes =
            defined_classes(&data).context("业务 DEX 类定义无效，无法安全检查壳类冲突")?;
        let mut conflicts: Vec<_> = classes.intersection(&reserved).collect();
        conflicts.sort();
        if let Some(name) = conflicts.first() {
            bail!(
                "业务 DEX 与壳的完整类名冲突：{}；为避免覆盖业务类，已停止加固",
                name.replace('/', ".")
            );
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dex_classes::fixture;
    use std::io::Write;

    fn resources(path: &Path) {
        let mut archive = zip::ZipWriter::new(fs::File::create(path).unwrap());
        archive
            .start_file("stub-classes.dex", zip::write::SimpleFileOptions::default())
            .unwrap();
        archive.write_all(&fixture("msk/a")).unwrap();
        archive.finish().unwrap();
    }

    #[test]
    fn 允许相同短类名但拒绝后续_dex_完整类名冲突() {
        let temp = tempfile::tempdir().unwrap();
        let apk = temp.path().join("apk");
        fs::create_dir(&apk).unwrap();
        let runtime = temp.path().join("resources.zip");
        resources(&runtime);
        fs::write(apk.join("classes.dex"), fixture("business/a")).unwrap();
        validate_class_identity(&apk, &runtime).unwrap();
        let collision = fixture("msk/a");
        fs::write(apk.join("classes2.dex"), &collision).unwrap();
        assert!(validate_class_identity(&apk, &runtime)
            .unwrap_err()
            .to_string()
            .contains("msk.a"));
        assert_eq!(fs::read(apk.join("classes2.dex")).unwrap(), collision);
    }

    #[test]
    fn 畸形业务或缺失壳不得被静默跳过() {
        let temp = tempfile::tempdir().unwrap();
        let runtime = temp.path().join("resources.zip");
        resources(&runtime);
        fs::write(temp.path().join("classes.dex"), b"invalid dex").unwrap();
        assert!(validate_class_identity(temp.path(), &runtime).is_err());
        assert!(validate_class_identity(temp.path(), &temp.path().join("missing.zip")).is_err());
    }
}

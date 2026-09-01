use anyhow::{Context, Result};
use std::fs;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

const JPEG_SIGNATURE: [u8; 3] = [0xFF, 0xD8, 0xFF];

/// 修正 aapt2 无法编译的“JPEG 内容伪装为 PNG”资源。
///
/// 仅处理解包目录的 `res`，并通过改名保留原始 JPEG 字节；九宫格资源和同名目标
/// 则拒绝处理，避免改变 Android 资源语义或覆盖用户文件。
pub(crate) fn normalize_mislabeled_jpeg_resources(apk_dir: &Path) -> Result<usize> {
    let resource_dir = apk_dir.join("res");
    if !resource_dir.is_dir() {
        return Ok(0);
    }

    let mut png_paths = Vec::<PathBuf>::new();
    for entry in WalkDir::new(&resource_dir).follow_links(false) {
        let entry =
            entry.with_context(|| format!("遍历资源目录失败：{}", resource_dir.display()))?;
        if entry.file_type().is_file() && is_png_extension(entry.path()) {
            png_paths.push(entry.into_path());
        }
    }

    let mut renamed = 0;
    for path in png_paths {
        if !has_jpeg_signature(&path)? {
            continue;
        }
        if is_nine_patch(&path) {
            anyhow::bail!(
                "检测到 JPEG 内容伪装为九宫格 PNG，无法安全自动修正：{}。请改为有效 .9.png 资源",
                path.display()
            );
        }
        let target = path.with_extension("jpg");
        if target.exists() {
            anyhow::bail!(
                "检测到 JPEG 内容伪装为 PNG，但同名 .jpg 已存在：{}。请手动处理资源冲突",
                target.display()
            );
        }
        fs::rename(&path, &target).with_context(|| {
            format!(
                "将 JPEG 伪装 PNG 资源改名失败：{} -> {}",
                path.display(),
                target.display()
            )
        })?;
        renamed += 1;
    }
    Ok(renamed)
}

fn is_png_extension(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("png"))
}

fn is_nine_patch(path: &Path) -> bool {
    path.file_stem()
        .and_then(|stem| stem.to_str())
        .is_some_and(|stem| stem.ends_with(".9"))
}

fn has_jpeg_signature(path: &Path) -> Result<bool> {
    let bytes = fs::read(path).with_context(|| format!("读取资源文件失败：{}", path.display()))?;
    Ok(bytes.starts_with(&JPEG_SIGNATURE))
}

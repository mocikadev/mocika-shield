use anyhow::{Context, Result};
use colored::Colorize;
use rand::RngCore;
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

use crate::error::ShieldError;
use crate::zipalign::align_apk;
use crate::utils::is_json_mode;
use crate::utils::{
    create_temp_dir, find_apktool, find_java, find_runtime_resources, human_size, print_step,
    print_success, run_command,
};

pub fn execute(input: &Path, output: &Path) -> Result<()> {
    let opts = ProtectOptions {
        input: input.to_path_buf(),
        output: output.to_path_buf(),
        apktool_path: None,
        resources_path: None,
    };

    protect_apk(&opts, |_| {}, Arc::new(AtomicBool::new(false)))?;
    Ok(())
}

#[derive(Debug, Clone)]
pub struct ProtectOptions {
    pub input: PathBuf,
    pub output: PathBuf,
    /// 用户自定义 apktool.jar 路径（优先于自动查找）
    pub apktool_path: Option<PathBuf>,
    /// 用户自定义 resources.zip 路径（优先于自动查找）
    pub resources_path: Option<PathBuf>,
}

#[derive(Debug, Clone)]
pub struct ProgressEvent {
    pub step: ProgressStep,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProgressStep {
    CheckTools,
    Unpack,
    ModifyManifest,
    ProcessDex,
    InjectRuntime,
    Repack,
    AlignApk,
}

pub fn protect_apk(
    opts: &ProtectOptions,
    on_progress: impl Fn(ProgressEvent) + Send + 'static,
    cancel: Arc<AtomicBool>,
) -> std::result::Result<(), ShieldError> {
    if !opts.input.exists() {
        return Err(ShieldError::FileNotFound(opts.input.display().to_string()));
    }

    let apktool = match &opts.apktool_path {
        Some(p) if p.exists() => p.clone(),
        Some(p) => {
            return Err(ShieldError::from(anyhow::anyhow!(
                "配置的 apktool.jar 路径不存在: {}",
                p.display()
            )))
        }
        None => find_apktool().map_err(ShieldError::from)?,
    };
    let runtime_resources = match &opts.resources_path {
        Some(p) if p.exists() => p.clone(),
        Some(p) => {
            return Err(ShieldError::from(anyhow::anyhow!(
                "配置的 resources.zip 路径不存在: {}",
                p.display()
            )))
        }
        None => find_runtime_resources().map_err(ShieldError::from)?,
    };

    if !is_json_mode() {
        println!("{}", "========================================".cyan());
        println!("{}", "Mocika Shield - APK Protection".cyan().bold());
        println!("{}", "========================================".cyan());
        println!("输入APK: {:?}", opts.input);
        println!("输出APK: {:?}", opts.output);
        println!("{}", "========================================".cyan());
    }

    emit_progress(&on_progress, &cancel, ProgressStep::CheckTools, "检查工具")?;
    print_step("检查工具");
    find_java().map_err(ShieldError::from)?;
    find_apktool().map_err(ShieldError::from)?;
    find_runtime_resources().map_err(ShieldError::from)?;
    print_success("所有工具就绪");

    let temp_dir = create_temp_dir("shield-").map_err(ShieldError::from)?;
    let apk_dir = temp_dir.path().join("apk");

    emit_progress(&on_progress, &cancel, ProgressStep::Unpack, "解包APK")?;
    print_step("解包APK");
    run_command(
        "java",
        &[
            "-jar",
            apktool.to_str().unwrap(),
            "d",
            opts.input.to_str().unwrap(),
            "-o",
            apk_dir.to_str().unwrap(),
            "-f",
            "--no-src",
        ],
        None,
    )
    .map_err(ShieldError::from)?;
    print_success("解包完成");

    let stub_app = read_stub_application(&runtime_resources).map_err(ShieldError::from)?;

    emit_progress(
        &on_progress,
        &cancel,
        ProgressStep::ModifyManifest,
        "修改AndroidManifest.xml",
    )?;
    print_step("修改AndroidManifest.xml");
    modify_manifest(&apk_dir, &stub_app).map_err(ShieldError::from)?;
    print_success("Manifest修改完成");

    emit_progress(
        &on_progress,
        &cancel,
        ProgressStep::ProcessDex,
        "处理DEX文件",
    )?;
    print_internal_step("提取APK签名");
    let signature = extract_apk_signature(&opts.input, &apktool).map_err(ShieldError::from)?;

    let mut ikm = [0u8; 32];
    rand::rng().fill_bytes(&mut ikm);

    print_step("处理DEX文件");
    process_dex(&apk_dir, &signature, &ikm).map_err(ShieldError::from)?;

    emit_progress(
        &on_progress,
        &cancel,
        ProgressStep::InjectRuntime,
        "注入Runtime库",
    )?;
    print_step("注入Runtime库");
    inject_runtime(&apk_dir, &runtime_resources, &opts.input).map_err(ShieldError::from)?;
    print_success("Runtime库注入完成");

    emit_progress(&on_progress, &cancel, ProgressStep::Repack, "重打包APK")?;
    print_step("重打包APK");
    run_command(
        "java",
        &[
            "-jar",
            apktool.to_str().unwrap(),
            "b",
            apk_dir.to_str().unwrap(),
            "-o",
            opts.output.to_str().unwrap(),
            "-f",
        ],
        None,
    )
    .map_err(ShieldError::from)?;

    let input_size = fs::metadata(&opts.input)
        .map_err(anyhow::Error::from)
        .map_err(ShieldError::from)?
        .len();
    let output_size = fs::metadata(&opts.output)
        .map_err(anyhow::Error::from)
        .map_err(ShieldError::from)?
        .len();
    let ratio = 100.0 * output_size as f64 / input_size as f64;
    print_success(&format!(
        "APK重打包完成: {} -> {} ({:.1}%)",
        human_size(input_size),
        human_size(output_size),
        ratio
    ));

    emit_progress(&on_progress, &cancel, ProgressStep::AlignApk, "对齐APK数据")?;
    print_step("对齐APK数据");
    align_apk(&opts.output).map_err(ShieldError::from)?;
    print_success("APK数据对齐完成");

    Ok(())
}

fn emit_progress<F>(
    on_progress: &F,
    cancel: &Arc<AtomicBool>,
    step: ProgressStep,
    message: &str,
) -> std::result::Result<(), ShieldError>
where
    F: Fn(ProgressEvent) + Send + 'static,
{
    check_cancel(cancel)?;
    on_progress(ProgressEvent {
        step,
        message: message.to_string(),
    });
    Ok(())
}

fn check_cancel(cancel: &Arc<AtomicBool>) -> std::result::Result<(), ShieldError> {
    if cancel.load(Ordering::Relaxed) {
        return Err(ShieldError::Cancelled);
    }

    Ok(())
}

/// 打印内部步骤标题，保持 CLI 现有输出不变。
fn print_internal_step(step: &str) {
    if !is_json_mode() {
        println!("\n{} {}", "=>".cyan().bold(), step.bold());
    }
}

fn modify_manifest(apk_dir: &Path, stub_app: &str) -> Result<()> {
    let manifest_path = apk_dir.join("AndroidManifest.xml");
    let content = fs::read_to_string(&manifest_path).context("读取 AndroidManifest.xml 失败")?;

    // 定位 <application 标签的起止范围（含属性，直到首个未被引号包裹的 >）
    let app_tag_start = content
        .find("<application")
        .context("AndroidManifest.xml 中未找到 <application> 标签")?;

    // 逐字节扫描找到开标签关闭位置，跳过引号内的 >
    let app_tag_end = find_tag_end(&content, app_tag_start)
        .context("AndroidManifest.xml <application> 标签未正常关闭")?;

    let app_tag = &content[app_tag_start..app_tag_end];

    // 提取原始 android:name（用于 ORIGINAL_APPLICATION meta-data）
    let orig_app = extract_xml_attr(app_tag, "android:name")
        .unwrap_or_else(|| "android.app.Application".to_string());
    log::info!("检测到原始 Application: {}", orig_app);

    let new_app_tag = set_xml_attr(app_tag, "android:name", stub_app);
    let new_app_tag = remove_xml_attr(&new_app_tag, "android:appComponentFactory");

    let already_injected = content.contains("android:name=\"ORIGINAL_APPLICATION\"");

    // 拼装 meta-data 注入块（缩进与 apktool 输出对齐：8 个空格）
    let meta_original = if already_injected {
        String::new()
    } else {
        format!(
            "\n        <meta-data\n            android:name=\"ORIGINAL_APPLICATION\"\n            android:value=\"{}\" />",
            orig_app
        )
    };

    // 判断标签是否自闭合（/> 结尾）——自闭合需展开为开放标签才能包含子元素
    let (prefix_tag, suffix) = if new_app_tag.trim_end().ends_with("/>") {
        let tag_body = new_app_tag
            .trim_end()
            .strip_suffix("/>")
            .unwrap_or(&new_app_tag)
            .trim_end_matches(|c: char| c.is_whitespace() || c == '/')
            .to_string();
        (
            format!("{}>", tag_body),
            meta_original + "\n    </application>",
        )
    } else {
        (new_app_tag.to_string(), meta_original)
    };

    let result = format!(
        "{}{}{}{}",
        &content[..app_tag_start],
        prefix_tag,
        suffix,
        &content[app_tag_end..]
    );

    fs::write(&manifest_path, result).context("写入 AndroidManifest.xml 失败")?;
    Ok(())
}

/// 从 content[start..] 扫描，返回开标签（`<tag ...>`）关闭后的字节偏移（即 `>` 之后一位）。
/// 跳过引号内包含的 `>`，避免将属性值中的 `>` 误判为标签结束。
fn find_tag_end(content: &str, start: usize) -> Option<usize> {
    let s = &content[start..];
    let mut in_quote: Option<u8> = None;
    for (i, &b) in s.as_bytes().iter().enumerate() {
        match in_quote {
            Some(q) if b == q => in_quote = None,
            Some(_) => {}
            None => match b {
                b'"' | b'\'' => in_quote = Some(b),
                b'>' => return Some(start + i + 1),
                _ => {}
            },
        }
    }
    None
}

/// 从 XML 开标签字符串中提取指定属性的值
fn extract_xml_attr(tag: &str, attr: &str) -> Option<String> {
    for &q in &[b'"', b'\''] {
        let needle = format!("{}={}", attr, q as char);
        if let Some(p) = tag.find(&needle) {
            let val_start = p + needle.len();
            let val_end = tag[val_start..].find(q as char)? + val_start;
            return Some(tag[val_start..val_end].to_string());
        }
    }
    None
}

/// 替换 XML 开标签中指定属性的值；属性不存在时在标签关闭前插入
fn set_xml_attr(tag: &str, attr: &str, value: &str) -> String {
    for &q in &[b'"', b'\''] {
        let needle = format!("{}={}", attr, q as char);
        if let Some(p) = tag.find(&needle) {
            let val_start = p + needle.len();
            let val_end = match tag[val_start..].find(q as char) {
                Some(e) => e + val_start,
                None => return tag.to_string(),
            };
            return format!(
                "{}{}{}\"{}\"{}",
                &tag[..p],
                attr,
                '=',
                value,
                &tag[val_end + 1..]
            );
        }
    }
    // 属性不存在：插到标签关闭符前
    let close = find_tag_end(tag, 0).unwrap_or(tag.len());
    format!(
        "{} {}=\"{}\"{}",
        &tag[..close - 1],
        attr,
        value,
        &tag[close - 1..]
    )
}

/// 从 XML 开标签字符串中移除指定属性及其值。
/// 保留属性前的分隔空格，删掉属性名到闭合引号的部分，再去掉紧随其后的多余空格。
fn remove_xml_attr(tag: &str, attr: &str) -> String {
    for &q in &[b'"', b'\''] {
        let needle = format!("{}={}", attr, q as char);
        if let Some(attr_start) = tag.find(&needle) {
            let val_start = attr_start + needle.len();
            let val_end = match tag[val_start..].find(q as char) {
                Some(e) => val_start + e + 1,
                None => continue,
            };
            // 去掉删除位置之后紧跟的多余空白，保持属性间单空格
            let rest = tag[val_end..].trim_start_matches(' ');
            // 若属性前有多个空格，保留一个作为分隔
            let prefix = &tag[..attr_start];
            let prefix = if prefix.ends_with(' ') {
                prefix
            } else {
                prefix.trim_end_matches(|c: char| c == ' ')
            };
            return format!("{}{}", prefix, rest);
        }
    }
    tag.to_string()
}

fn process_dex(apk_dir: &Path, signature: &str, ikm: &[u8]) -> Result<()> {
    let mut dex_files = Vec::new();
    for entry in fs::read_dir(apk_dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) == Some("dex") {
            dex_files.push(path);
        }
    }

    if dex_files.is_empty() {
        anyhow::bail!("未找到DEX文件");
    }

    let dex_dir = apk_dir.parent().unwrap().join("dex");
    fs::create_dir_all(&dex_dir)?;

    for dex_file in &dex_files {
        let dest = dex_dir.join(dex_file.file_name().unwrap());
        fs::rename(dex_file, dest)?;
    }

    let assets_dir = apk_dir.join("assets");
    fs::create_dir_all(&assets_dir)?;
    let routes = crate::dex_packer::route_scanner::scan_arouter_routes(&dex_dir)?;
    if !routes.is_empty() {
        let route_file = assets_dir.join("arouter_routes.txt");
        fs::write(&route_file, routes.join("\n"))?;
        print_success(&format!("扫描到 ARouter 路由表类 {} 个", routes.len()));
    }

    // 打包到临时路径，inject_runtime 阶段追加到 classes.dex 末尾
    let tmp_bin = apk_dir.parent().unwrap().join("app.bin.tmp");
    crate::dex_packer::pack_dex_files(&dex_dir, &tmp_bin, ikm, signature)?;

    let bin_size = fs::metadata(&tmp_bin)?.len();
    print_success(&format!("DEX打包完成: {}", human_size(bin_size)));

    Ok(())
}

fn inject_runtime(apk_dir: &Path, runtime_resources: &Path, original_apk: &Path) -> Result<()> {
    let supported_abis = collect_apk_abis(original_apk);

    let file = fs::File::open(runtime_resources)?;
    let mut archive = zip::ZipArchive::new(file)?;

    for i in 0..archive.len() {
        let mut file = archive.by_index(i)?;
        let file_name = file.name().to_string();

        if file_name.contains("libzstd-jni") {
            continue;
        }

        if !supported_abis.is_empty() && file_name.starts_with("lib/") {
            let abi = file_name.splitn(3, '/').nth(1).unwrap_or("");
            if !abi.is_empty() && !supported_abis.contains(abi) {
                continue;
            }
        }

        let outpath = apk_dir.join(&file_name);
        if file_name.ends_with('/') {
            fs::create_dir_all(&outpath)?;
        } else {
            if let Some(p) = outpath.parent() {
                fs::create_dir_all(p)?;
            }
            let mut outfile = fs::File::create(&outpath)?;
            std::io::copy(&mut file, &mut outfile)?;
        }
    }

    let stub_dex = apk_dir.join("stub-classes.dex");
    let classes_dex = apk_dir.join("classes.dex");

    if stub_dex.exists() {
        fs::rename(&stub_dex, &classes_dex).context("重命名stub-classes.dex失败")?;
        print_success("stub-classes.dex -> classes.dex");
    } else {
        anyhow::bail!("未找到stub-classes.dex");
    }

    // 将加密 DEX 数据追加到 classes.dex 末尾（MSHD magic + 长度 + payload）
    // DEX 解析工具读到 file_size 后停止，末尾追加内容对工具不可见
    let tmp_bin = apk_dir.parent().unwrap().join("app.bin.tmp");
    if !tmp_bin.exists() {
        anyhow::bail!("未找到临时 app.bin.tmp，process_dex 可能未执行");
    }
    let payload = fs::read(&tmp_bin).context("读取 app.bin.tmp 失败")?;
    if let Err(e) = fs::remove_file(&tmp_bin) {
        log::warn!("删除临时文件 app.bin.tmp 失败（不影响加固结果）: {}", e);
    }

    // 校验 payload 大小：u32 最大 ~4GiB，实际 DEX 远不可能到达此上限
    let payload_len =
        u32::try_from(payload.len()).context("payload 超过 4GiB 上限，无法写入 u32 长度字段")?;

    // 追加前确保 classes.dex 不含旧的 MSHD 尾部（防止重复加固产生多份 MSHD）
    // DEX 标准格式在头部 offset 8 写入 file_size（u32 LE），截断到此即回到原始 DEX 边界
    let original_dex_size = {
        let header = fs::read(&classes_dex).context("读取 classes.dex header 失败")?;
        if header.len() < 12 {
            anyhow::bail!("classes.dex 过短，无法读取 DEX file_size 字段");
        }
        u32::from_le_bytes(header[8..12].try_into().unwrap()) as u64
    };
    let current_size = fs::metadata(&classes_dex)?.len();
    if current_size > original_dex_size {
        // 文件比 DEX header 声称的 file_size 更大，说明已有尾部追加数据，裁剪掉
        let file = fs::OpenOptions::new()
            .write(true)
            .open(&classes_dex)
            .context("打开 classes.dex 裁剪失败")?;
        file.set_len(original_dex_size)
            .context("裁剪 classes.dex 到原始 file_size 失败")?;
        log::warn!(
            "classes.dex 已有尾部追加数据（当前 {} 字节，原始 {} 字节），已裁剪",
            current_size,
            original_dex_size
        );
    }

    let mut classes_dex_file = fs::OpenOptions::new()
        .append(true)
        .open(&classes_dex)
        .context("打开 classes.dex 追加写入失败")?;
    use std::io::Write;
    classes_dex_file
        .write_all(b"MSHD")
        .context("写入 MSHD magic 失败")?;
    classes_dex_file
        .write_all(&payload_len.to_le_bytes())
        .context("写入 payload 长度失败")?;
    classes_dex_file
        .write_all(&payload)
        .context("写入 payload 失败")?;

    drop(classes_dex_file);
    patch_dex_header(&classes_dex).context("修复 classes.dex DEX header 失败")?;

    let classes_dex_size = fs::metadata(&classes_dex)?.len();
    print_success(&format!(
        "加密 DEX 已追加到 classes.dex 末尾（总大小 {}）",
        human_size(classes_dex_size)
    ));
    print_success("Runtime资源注入完成");
    Ok(())
}

/// 追加 MSHD payload 后，将 classes.dex 的 DEX header 更新为实际文件大小并重算校验值。
///
/// DEX header 布局（字节偏移）：
///   [0..8]   magic
///   [8..12]  checksum  — Adler32([12..file_end])
///   [12..32] signature — SHA-1([32..file_end])
///   [32..36] file_size — 整个文件的字节数
///
/// ART 在加载 APK 内的 DEX 时会严格校验：实际文件大小 == header.file_size，
/// 否则报 "Bad file size" 并拒绝解析，导致 ClassNotFoundException。
fn patch_dex_header(dex_path: &Path) -> Result<()> {
    use sha1::{Digest, Sha1};
    use std::io::{Read, Seek, SeekFrom, Write};

    let mut file = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(dex_path)
        .context("打开 classes.dex 读写失败")?;

    let mut data = Vec::new();
    file.read_to_end(&mut data)
        .context("读取 classes.dex 全量失败")?;

    if data.len() < 36 {
        anyhow::bail!("classes.dex 过短，无法修复 header（{}字节）", data.len());
    }

    let total_len =
        u32::try_from(data.len()).context("classes.dex 超过 4GiB，无法写入 file_size")?;

    // 更新 file_size 字段 [32..36]
    data[32..36].copy_from_slice(&total_len.to_le_bytes());

    // 重算 SHA-1 signature（覆盖 [32..end]），写入 [12..32]
    let sig = Sha1::digest(&data[32..]);
    data[12..32].copy_from_slice(&sig);

    // 重算 Adler32 checksum（覆盖 [12..end]），写入 [8..12]
    let checksum = adler32_checksum(&data[12..]);
    data[8..12].copy_from_slice(&checksum.to_le_bytes());

    file.seek(SeekFrom::Start(0))
        .context("定位 classes.dex 写入位置失败")?;
    file.write_all(&data)
        .context("回写 classes.dex header 失败")?;

    Ok(())
}

/// DEX 格式规定的 Adler32 实现（RFC 1950，modulus 65521）
fn adler32_checksum(data: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    for &byte in data {
        a = (a + byte as u32) % 65521;
        b = (b + a) % 65521;
    }
    (b << 16) | a
}

fn collect_apk_abis(apk_path: &Path) -> HashSet<String> {
    let mut abis = HashSet::new();
    let Ok(file) = fs::File::open(apk_path) else {
        return abis;
    };
    let Ok(mut archive) = zip::ZipArchive::new(file) else {
        return abis;
    };
    for i in 0..archive.len() {
        let Ok(entry) = archive.by_index(i) else {
            continue;
        };
        let name = entry.name();
        if name.starts_with("lib/") && name.len() > 4 {
            if let Some(abi) = name.splitn(3, '/').nth(1) {
                if !abi.is_empty() {
                    abis.insert(abi.to_string());
                }
            }
        }
    }
    abis
}

fn extract_apk_signature(apk_path: &Path, apktool: &Path) -> Result<String> {
    let temp_dir = create_temp_dir("sig-extract-")?;
    let extract_dir = temp_dir.path().join("apk");

    run_command(
        "java",
        &[
            "-jar",
            apktool.to_str().unwrap(),
            "d",
            apk_path.to_str().unwrap(),
            "-o",
            extract_dir.to_str().unwrap(),
            "-f",
            "--no-src",
            "--no-res",
        ],
        None,
    )?;

    let meta_inf = extract_dir.join("original").join("META-INF");

    if !meta_inf.exists() {
        log::warn!("未找到META-INF目录，APK可能未签名或只有V2/V3签名");
        return extract_signature_via_keytool(apk_path);
    }

    for entry in fs::read_dir(&meta_inf)? {
        let entry = entry?;
        let path = entry.path();
        let filename = path.file_name().unwrap().to_str().unwrap();

        if filename.ends_with(".RSA") || filename.ends_with(".DSA") || filename.ends_with(".EC") {
            match extract_certificate_sha256_via_java(&path) {
                Ok(signature) => {
                    print_success(&format!("原始APK签名(V1证书): {}...", &signature[..16]));
                    return Ok(signature);
                }
                Err(e) => {
                    log::warn!("Java证书提取失败: {}, 尝试keytool", e);
                    return extract_signature_via_keytool(apk_path);
                }
            }
        }
    }

    log::warn!("未找到签名文件(.RSA/.DSA/.EC)，尝试keytool");
    extract_signature_via_keytool(apk_path)
}

fn extract_certificate_sha256_via_java(cert_file: &Path) -> Result<String> {
    let java_code = r#"
import java.io.*;
import java.security.cert.*;
import java.security.MessageDigest;
import java.util.Collection;

public class CertExtractor {
    public static void main(String[] args) throws Exception {
        FileInputStream fis = new FileInputStream(args[0]);
        CertificateFactory cf = CertificateFactory.getInstance("X.509");
        Collection<? extends Certificate> certs = cf.generateCertificates(fis);
        fis.close();
        if (certs.isEmpty()) { System.err.println("No certificates found"); System.exit(1); }
        X509Certificate cert = (X509Certificate) certs.iterator().next();
        MessageDigest md = MessageDigest.getInstance("SHA-256");
        byte[] digest = md.digest(cert.getEncoded());
        StringBuilder hexString = new StringBuilder();
        for (byte b : digest) {
            String hex = Integer.toHexString(0xff & b);
            if (hex.length() == 1) hexString.append('0');
            hexString.append(hex);
        }
        System.out.print(hexString.toString().toUpperCase());
    }
}
    "#;

    let temp_dir = tempfile::tempdir()?;
    let java_file = temp_dir.path().join("CertExtractor.java");
    fs::write(&java_file, java_code)?;

    let javac_output = no_window_command("javac")
        .arg(java_file.to_str().unwrap())
        .current_dir(temp_dir.path())
        .output()?;

    if !javac_output.status.success() {
        anyhow::bail!(
            "javac 编译失败: {}",
            String::from_utf8_lossy(&javac_output.stderr)
        );
    }

    let java_output = no_window_command("java")
        .arg("-cp")
        .arg(temp_dir.path().to_str().unwrap())
        .arg("CertExtractor")
        .arg(cert_file.to_str().unwrap())
        .output()?;

    if !java_output.status.success() {
        anyhow::bail!(
            "CertExtractor 执行失败: {}",
            String::from_utf8_lossy(&java_output.stderr)
        );
    }

    let signature = String::from_utf8(java_output.stdout)?;
    Ok(signature.trim().to_string())
}

fn extract_signature_via_keytool(apk_path: &Path) -> Result<String> {
    log::info!("使用keytool提取证书...");

    let output = no_window_command("keytool")
        .arg("-printcert")
        .arg("-jarfile")
        .arg(apk_path.to_str().unwrap())
        .output();

    if let Ok(output) = output {
        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            for line in stdout.lines() {
                if line.trim().starts_with("SHA256:") {
                    let fingerprint = line.split(':').skip(1).collect::<String>();
                    let clean = fingerprint.replace(':', "").replace(' ', "").to_uppercase();
                    if clean.len() == 64 {
                        print_success(&format!("原始APK签名(V2/V3证书): {}...", &clean[..16]));
                        return Ok(clean);
                    }
                }
            }
        }
    }

    anyhow::bail!(
        "签名提取失败：所有提取方式均未成功。\n\
         请确认输入 APK 已签名，或使用 keytool/apksigner 检查签名状态。\n\
         如需加固未签名 APK，请先对其签名后再执行加固。"
    )
}

fn no_window_command(prog: &str) -> std::process::Command {
    #[allow(unused_mut)]
    let mut cmd = std::process::Command::new(prog);
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

/// 从 resources.zip 内的 metadata.json 读取 stub_application 字段（混淆后的壳类名）。
fn read_stub_application(resources_path: &Path) -> Result<String> {
    use std::io::Read;

    let file = fs::File::open(resources_path).context("打开 resources.zip 失败")?;
    let mut archive = zip::ZipArchive::new(file).context("解析 resources.zip 失败")?;
    let mut entry = archive
        .by_name("metadata.json")
        .context("resources.zip 中未找到 metadata.json")?;
    let mut content = String::new();
    entry
        .read_to_string(&mut content)
        .context("读取 metadata.json 失败")?;

    parse_json_string_field(&content, "stub_application")
        .context("metadata.json 中未找到 stub_application 字段")
}

/// 从 JSON 文本中提取指定字符串字段的值（不引入 serde_json 依赖）。
fn parse_json_string_field(json: &str, field: &str) -> Option<String> {
    let needle = format!("\"{}\"", field);
    let start = json.find(&needle)?;
    let after_key = &json[start + needle.len()..];
    let after_colon = after_key.trim_start().strip_prefix(':')?.trim_start();
    let after_quote = after_colon.strip_prefix('"')?;
    let end = after_quote.find('"')?;
    Some(after_quote[..end].to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write as _;

    #[test]
    fn find_tag_end_simple_tag() {
        let s = r#"<application android:label="app">"#;
        assert_eq!(find_tag_end(s, 0), Some(s.len()));
    }

    #[test]
    fn find_tag_end_self_closing() {
        let s = r#"<application />"#;
        assert_eq!(find_tag_end(s, 0), Some(s.len()));
    }

    #[test]
    fn find_tag_end_gt_inside_quoted_attr_skipped() {
        let s = r#"<application android:label="a>b">"#;
        assert_eq!(find_tag_end(s, 0), Some(s.len()));
    }

    #[test]
    fn find_tag_end_with_nonzero_start() {
        let s = "HEADER<application>";
        assert_eq!(find_tag_end(s, 6), Some(s.len()));
    }

    #[test]
    fn extract_xml_attr_double_quoted() {
        let tag = r#"<application android:name="com.example.App">"#;
        assert_eq!(
            extract_xml_attr(tag, "android:name"),
            Some("com.example.App".to_string())
        );
    }

    #[test]
    fn extract_xml_attr_not_found_returns_none() {
        let tag = r#"<application android:label="app">"#;
        assert_eq!(extract_xml_attr(tag, "android:name"), None);
    }

    #[test]
    fn set_xml_attr_replaces_existing_value() {
        let tag = r#"<application android:name="com.example.App">"#;
        let result = set_xml_attr(tag, "android:name", "dev.mocika.StubApp");
        assert!(result.contains(r#"android:name="dev.mocika.StubApp""#));
        assert!(!result.contains("com.example.App"));
    }

    #[test]
    fn set_xml_attr_inserts_when_absent() {
        let tag = r#"<application android:label="app">"#;
        let result = set_xml_attr(tag, "android:name", "dev.mocika.StubApp");
        assert!(result.contains(r#"android:name="dev.mocika.StubApp""#));
    }

    #[test]
    fn remove_xml_attr_removes_found_attr() {
        let tag = r#"<application android:appComponentFactory="abc" android:name="App">"#;
        let result = remove_xml_attr(tag, "android:appComponentFactory");
        assert!(!result.contains("android:appComponentFactory"));
        assert!(result.contains(r#"android:name="App""#));
    }

    #[test]
    fn remove_xml_attr_not_found_returns_original() {
        let tag = r#"<application android:name="App">"#;
        let result = remove_xml_attr(tag, "android:nonExistent");
        assert_eq!(result, tag);
    }

    #[test]
    fn modify_manifest_replaces_application_name_and_injects_meta() {
        let dir = tempfile::tempdir().unwrap();
        let manifest = concat!(
            r#"<?xml version="1.0" encoding="utf-8"?>"#,
            "\n<manifest package=\"com.example\">\n",
            "    <application android:name=\"com.example.App\">\n",
            "    </application>\n</manifest>"
        );
        fs::write(dir.path().join("AndroidManifest.xml"), manifest).unwrap();
        modify_manifest(dir.path(), "dev.mocika.StubApp").unwrap();
        let result = fs::read_to_string(dir.path().join("AndroidManifest.xml")).unwrap();
        assert!(result.contains(r#"android:name="dev.mocika.StubApp""#));
        assert!(result.contains("ORIGINAL_APPLICATION"));
        assert!(result.contains("com.example.App"));
    }

    #[test]
    fn modify_manifest_self_closing_application_expanded() {
        let dir = tempfile::tempdir().unwrap();
        let manifest = concat!(
            r#"<?xml version="1.0" encoding="utf-8"?>"#,
            "\n<manifest package=\"com.example\">\n",
            "    <application android:name=\"com.example.App\" />\n",
            "</manifest>"
        );
        fs::write(dir.path().join("AndroidManifest.xml"), manifest).unwrap();
        modify_manifest(dir.path(), "dev.mocika.StubApp").unwrap();
        let result = fs::read_to_string(dir.path().join("AndroidManifest.xml")).unwrap();
        assert!(result.contains(r#"android:name="dev.mocika.StubApp""#));
        assert!(result.contains("</application>"));
        assert!(result.contains("ORIGINAL_APPLICATION"));
    }

    #[test]
    fn modify_manifest_already_injected_no_duplicate_meta() {
        let dir = tempfile::tempdir().unwrap();
        let manifest = concat!(
            r#"<?xml version="1.0" encoding="utf-8"?>"#,
            "\n<manifest package=\"com.example\">\n",
            "    <application android:name=\"dev.mocika.StubApp\">\n",
            "        <meta-data android:name=\"ORIGINAL_APPLICATION\"",
            " android:value=\"com.example.App\" />\n",
            "    </application>\n</manifest>"
        );
        fs::write(dir.path().join("AndroidManifest.xml"), manifest).unwrap();
        modify_manifest(dir.path(), "dev.mocika.StubApp").unwrap();
        let result = fs::read_to_string(dir.path().join("AndroidManifest.xml")).unwrap();
        assert_eq!(result.matches("ORIGINAL_APPLICATION").count(), 1);
    }

    #[test]
    fn adler32_empty_input_returns_one() {
        assert_eq!(adler32_checksum(b""), 1);
    }

    #[test]
    fn adler32_wikipedia_known_value() {
        // RFC 1950 参考值：Adler32("Wikipedia") = 0x11E60398
        assert_eq!(adler32_checksum(b"Wikipedia"), 0x11E60398);
    }

    #[test]
    fn parse_json_string_field_found() {
        let json = r#"{"stub_application": "msk.b", "version": "5"}"#;
        assert_eq!(
            parse_json_string_field(json, "stub_application"),
            Some("msk.b".to_string())
        );
    }

    #[test]
    fn parse_json_string_field_not_found_returns_none() {
        let json = r#"{"version": "5"}"#;
        assert_eq!(parse_json_string_field(json, "stub_application"), None);
    }

    #[test]
    fn collect_apk_abis_detects_multiple_abis() {
        let dir = tempfile::tempdir().unwrap();
        let apk_path = dir.path().join("test.apk");
        {
            let f = fs::File::create(&apk_path).unwrap();
            let mut zip = zip::ZipWriter::new(f);
            let opts = zip::write::SimpleFileOptions::default();
            zip.start_file("lib/arm64-v8a/libtest.so", opts).unwrap();
            zip.write_all(b"elf").unwrap();
            zip.start_file("lib/armeabi-v7a/libtest.so", opts).unwrap();
            zip.write_all(b"elf").unwrap();
            zip.start_file("classes.dex", opts).unwrap();
            zip.write_all(b"dex").unwrap();
            zip.finish().unwrap();
        }
        let abis = collect_apk_abis(&apk_path);
        assert!(abis.contains("arm64-v8a"));
        assert!(abis.contains("armeabi-v7a"));
        assert!(!abis.contains("x86"));
    }

    #[test]
    fn collect_apk_abis_no_lib_dir_returns_empty() {
        let dir = tempfile::tempdir().unwrap();
        let apk_path = dir.path().join("test.apk");
        {
            let f = fs::File::create(&apk_path).unwrap();
            let mut zip = zip::ZipWriter::new(f);
            let opts = zip::write::SimpleFileOptions::default();
            zip.start_file("classes.dex", opts).unwrap();
            zip.write_all(b"dex").unwrap();
            zip.finish().unwrap();
        }
        let abis = collect_apk_abis(&apk_path);
        assert!(abis.is_empty());
    }

    #[test]
    fn collect_apk_abis_nonexistent_path_returns_empty() {
        let abis = collect_apk_abis(std::path::Path::new("/nonexistent/path.apk"));
        assert!(abis.is_empty());
    }
}

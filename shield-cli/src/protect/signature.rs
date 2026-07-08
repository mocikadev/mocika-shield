use anyhow::Result;
use std::fs;
use std::path::Path;

use crate::utils::{create_temp_dir, no_window_command, print_success, run_command};

pub(crate) fn extract_apk_signature(apk_path: &Path, apktool: &Path) -> Result<String> {
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

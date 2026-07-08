use crate::app_config::normalize_keystore_type;
use crate::app_paths::find_apksigner_path;
use shield_core::{
    sign_apk as shield_sign_apk,
    utils::{find_keytool, no_window_command},
    KeystoreType, SignOptions, SigningVersions,
};
use std::path::PathBuf;

pub(crate) fn execute_sign_apk(
    app: &tauri::AppHandle,
    keystore_password: String,
    key_password: String,
    apk_path: String,
    output_path: Option<String>,
    apksigner_path: Option<String>,
    keystore_path: String,
    key_alias: String,
    ks_type: Option<String>,
    sign_v1: bool,
    sign_v2: bool,
    sign_v3: bool,
    sign_v4: bool,
) -> Result<(), String> {
    let resolved_apksigner = apksigner_path
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .or_else(|| find_apksigner_path(app));
    let ks_pass = keystore_password;
    let key_pass = key_password;
    let effective_key_pass = if key_pass.is_empty() {
        ks_pass.clone()
    } else {
        key_pass
    };
    let opts = SignOptions {
        apk_path: PathBuf::from(apk_path),
        output_path: output_path.filter(|s| !s.is_empty()).map(PathBuf::from),
        keystore_path: PathBuf::from(keystore_path),
        key_alias,
        keystore_password: ks_pass,
        key_password: effective_key_pass,
        apksigner_path: resolved_apksigner,
        keystore_type: KeystoreType::from_str(
            normalize_keystore_type(ks_type.as_deref())
                .as_deref()
                .unwrap_or("JKS"),
        ),
        signing_versions: SigningVersions {
            v1: sign_v1,
            v2: sign_v2,
            v3: sign_v3,
            v4: sign_v4,
        },
    };
    shield_sign_apk(&opts).map_err(|e| e.to_string())
}

pub(crate) fn query_keystore_aliases(
    keystore_path: String,
    ks_pass: String,
    ks_type: Option<String>,
) -> Result<Vec<String>, String> {
    let ks_type_str = ks_type.as_deref().unwrap_or("JKS");
    let keytool = find_keytool().map_err(|err| err.to_string())?;
    let output = no_window_command(&keytool)
        .args([
            "-list",
            "-keystore",
            &keystore_path,
            "-storetype",
            ks_type_str,
            "-storepass",
            &ks_pass,
        ])
        .output()
        .map_err(|e| format!("启动 keytool 失败，请确认 JDK 17+ 已安装: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("keytool 执行失败: {}", stderr.trim()));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let aliases = parse_keytool_aliases(&stdout);

    if aliases.is_empty() {
        Err("未在 keystore 中找到任何 alias".to_string())
    } else {
        Ok(aliases)
    }
}

fn parse_keytool_aliases(output: &str) -> Vec<String> {
    let mut aliases = Vec::new();
    for line in output.lines() {
        let trimmed = line.trim();
        if trimmed.contains("PrivateKeyEntry")
            || trimmed.contains("SecretKeyEntry")
            || trimmed.contains("trustedCertEntry")
        {
            if let Some(alias) = trimmed.split(',').next() {
                let alias = alias.trim().to_string();
                if !alias.is_empty() {
                    aliases.push(alias);
                }
            }
        }
    }
    aliases
}

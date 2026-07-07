fn main() {
    let vars: &[(&str, &str)] = &[
        // JVM 内部路径格式用斜线，如 "dev/mocika/shield/loader/Ld"
        ("STUB_BINLOADER_CLASS", "dev/mocika/shield/loader/Ld"),
        ("STUB_METHOD_INJECT_DEX", "p"),
        ("STUB_METHOD_EXTRACT_DECRYPT", "q"),
        ("STUB_METHOD_GET_SIG", "getSignatureSha256"),
    ];

    for (key, default) in vars {
        // 环境变量变化时让 Cargo 重新运行 build.rs，确保 .so 与 DEX 保持一致
        println!("cargo:rerun-if-env-changed={key}");
        let value = std::env::var(key).unwrap_or_else(|_| default.to_string());
        println!("cargo:rustc-env={key}={value}");
    }
}

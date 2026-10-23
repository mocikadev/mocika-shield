fn main() {
    let vars: &[(&str, &str)] = &[
        // JVM 内部路径格式用斜线，如 "dev/mocika/shield/loader/Ld"
        ("STUB_BINLOADER_CLASS", "dev/mocika/shield/loader/Ld"),
        ("STUB_METHOD_INJECT_DEX", "p"),
        ("STUB_METHOD_EXTRACT_DECRYPT", "q"),
        ("STUB_METHOD_CHECK_ENV", "r"),
        ("STUB_METHOD_DECRYPT_BUFFERS", "s"),
        ("STUB_METHOD_GET_SIG", "getSignatureSha256"),
        (
            "STUB_SIGNATURE_INJECT_DEX",
            "(Ljava/lang/ClassLoader;[Ljava/lang/String;Ljava/lang/String;)Z",
        ),
        (
            "STUB_SIGNATURE_EXTRACT_DECRYPT",
            "(Landroid/content/Context;[B)[[B",
        ),
        ("STUB_SIGNATURE_CHECK_ENV", "()I"),
        (
            "STUB_SIGNATURE_DECRYPT_BUFFERS",
            "(Landroid/content/Context;[B)[Ljava/nio/ByteBuffer;",
        ),
        (
            "STUB_SIGNATURE_GET_SIG",
            "(Landroid/content/Context;)Ljava/lang/String;",
        ),
        ("STUB_BINDING_ID", "development"),
    ];

    println!("cargo:rerun-if-env-changed=STUB_BINDINGS_REQUIRED");
    let required = std::env::var("STUB_BINDINGS_REQUIRED").as_deref() == Ok("1");
    for (key, default) in vars {
        // 环境变量变化时让 Cargo 重新运行 build.rs，确保 .so 与 DEX 保持一致
        println!("cargo:rerun-if-env-changed={key}");
        let value = std::env::var(key).unwrap_or_else(|_| {
            assert!(!required, "正式壳构建缺少绑定变量：{key}");
            default.to_string()
        });
        assert!(!value.is_empty(), "壳绑定变量不能为空：{key}");
        println!("cargo:rustc-env={key}={value}");
    }
}

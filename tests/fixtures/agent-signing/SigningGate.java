package com.android.apksigner;

import java.lang.reflect.InvocationTargetException;
import java.net.URL;
import java.net.URLClassLoader;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.Paths;

/** 测试模板；构建时改名为固定主类，只在原工具实际返回后保持确定的 JVM 等待窗口。 */
public final class ApkSignerTool {
    public static void main(String[] args) throws Exception {
        if (args.length < 1 || !(args[0].equals("sign") || args[0].equals("verify"))) {
            throw new IllegalArgumentException();
        }
        String operation = args[0];
        Path root = Paths.get(System.getenv("MOCIKA_TEST_JAVA_ROOT"));
        if (System.in.read() != -1) {
            throw new IllegalStateException("业务工具不得获得控制租约");
        }
        mark(root, operation + "-java-eof");
        URL original = Paths.get(System.getenv("MOCIKA_TEST_REAL_SIGNER")).toUri().toURL();
        // 使用独立加载器调用原 JAR，不能递归调用此测试模板或替换原签名算法。
        URLClassLoader loader = new URLClassLoader(new URL[] {original},
            ClassLoader.getSystemClassLoader().getParent());
        ClassLoader previous = Thread.currentThread().getContextClassLoader();
        Thread.currentThread().setContextClassLoader(loader);
        mark(root, operation + "-real-entered");
        try {
            Class<?> entry = Class.forName("com.android.apksigner.ApkSignerTool", true, loader);
            if (entry.getClassLoader() != loader) {
                throw new IllegalStateException();
            }
            entry.getMethod("main", String[].class).invoke(null, (Object) args);
        } catch (InvocationTargetException error) {
            // 原工具主动退出的情况由代理记录；不回显异常原文。
            System.exit(42);
        } finally {
            Thread.currentThread().setContextClassLoader(previous);
        }
        Files.write(root.resolve(operation + "-status"), "0".getBytes(StandardCharsets.US_ASCII));
        mark(root, operation + "-real-completed");
        if ((operation + "-jar").equals(System.getenv("MOCIKA_TEST_SIGN_PHASE"))) {
            ProcessBuilder builder = new ProcessBuilder(System.getenv("MOCIKA_TEST_SIGN_PROXY"), "--writer");
            builder.redirectInput(ProcessBuilder.Redirect.INHERIT);
            // 等待窗口故意保持原管道；外层必须停止 JVM 及其后代才能得到 EOF。
            builder.redirectOutput(ProcessBuilder.Redirect.INHERIT);
            builder.redirectError(ProcessBuilder.Redirect.INHERIT);
            builder.environment().remove("MOCIKA_SHIELD_TOOL_KS_PASS");
            builder.environment().remove("MOCIKA_SHIELD_TOOL_KEY_PASS");
            Process writer = builder.start();
            mark(root, operation + "-jar-ready");
            if (writer.waitFor() != 0) {
                throw new IllegalStateException();
            }
            throw new IllegalStateException("JVM 等待窗口必须由外层停止");
        }
    }

    private static void mark(Path root, String name) throws Exception {
        Path pending = root.resolve("." + name + ".pending");
        Files.write(pending, "yes".getBytes(StandardCharsets.US_ASCII));
        Files.move(pending, root.resolve(name));
    }
}

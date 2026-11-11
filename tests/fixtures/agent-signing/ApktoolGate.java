package brut.apktool;

import java.net.URL;
import java.net.URLClassLoader;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.Paths;
import java.util.ArrayList;
import java.util.Arrays;

/** 测试固定主类；委托原 apktool，仅测试重建时指定原生代理，不进入正式 JAR。 */
public final class Main {
    public static void main(String[] args) throws Exception {
        if (args.length == 0 || !(args[0].equals("d") || args[0].equals("b"))) {
            throw new IllegalArgumentException();
        }
        Path root = Paths.get(System.getenv("MOCIKA_TEST_JAVA_ROOT"));
        Path temporary = Paths.get(System.getProperty("java.io.tmpdir")).toRealPath();
        if (!temporary.startsWith(root.resolve("中文 工作").toRealPath())) {
            throw new IllegalStateException("Native 临时文件不得落入全局缓存");
        }
        if (System.in.read() != -1) {
            throw new IllegalStateException("助手请求不得混用控制租约");
        }
        if ("flood".equals(System.getenv("MOCIKA_TEST_APKTOOL_MODE"))) {
            // 原 apktool 的转发线程不保证在失败退出前排完 Native 日志。
            // 只在真实 Native 调用及超量写入已确认后，由测试 JVM 确定地检验核心预算。
            Runtime.getRuntime().addShutdownHook(new Thread(() -> {
                try {
                    if (!Files.exists(root.resolve("aapt-completed"))
                            || !Files.exists(root.resolve("apktool-flood-written"))) {
                        return;
                    }
                    byte[] bytes = new byte[1024 * 1024];
                    Arrays.fill(bytes, (byte) 'x');
                    System.out.write(bytes);
                    System.err.write(bytes);
                    System.out.flush();
                    System.err.flush();
                    mark(root, "java-flood-written");
                } catch (Exception error) {
                    throw new IllegalStateException("测试 JVM 排空未完成");
                }
            }));
        }
        mark(root, args[0] + "-java-eof");
        ArrayList<String> values = new ArrayList<String>(Arrays.asList(args));
        if (args[0].equals("b")) {
            values.add("--aapt");
            values.add(System.getenv("MOCIKA_TEST_APKTOOL_PROXY"));
        }
        URL original = Paths.get(System.getenv("MOCIKA_TEST_REAL_APKTOOL")).toUri().toURL();
        URLClassLoader loader = new URLClassLoader(new URL[] {original},
            ClassLoader.getSystemClassLoader().getParent());
        ClassLoader previous = Thread.currentThread().getContextClassLoader();
        Thread.currentThread().setContextClassLoader(loader);
        mark(root, args[0] + "-real-entered");
        try {
            Class<?> entry = Class.forName("brut.apktool.Main", true, loader);
            if (entry.getClassLoader() != loader) {
                throw new IllegalStateException();
            }
            entry.getMethod("main", String[].class).invoke(null, (Object) values.toArray(new String[0]));
        } finally {
            Thread.currentThread().setContextClassLoader(previous);
        }
        mark(root, args[0] + "-real-completed");
        if (args[0].equals("d") && "decode".equals(System.getenv("MOCIKA_TEST_APKTOOL_PHASE"))) {
            ProcessBuilder builder = new ProcessBuilder(System.getenv("MOCIKA_TEST_APKTOOL_PROXY"), "--decode-wait");
            builder.redirectInput(ProcessBuilder.Redirect.INHERIT);
            builder.redirectOutput(ProcessBuilder.Redirect.INHERIT);
            builder.redirectError(ProcessBuilder.Redirect.INHERIT);
            builder.environment().remove("MOCIKA_SHIELD_TOOL_KS_PASS");
            builder.environment().remove("MOCIKA_SHIELD_TOOL_KEY_PASS");
            if (builder.start().waitFor() != 0) {
                throw new IllegalStateException();
            }
        }
    }

    private static void mark(Path root, String name) throws Exception {
        Files.write(root.resolve(name), "yes".getBytes(StandardCharsets.US_ASCII));
    }
}

package dev.mocika.shield.tools;

import java.io.PrintStream;
import java.lang.reflect.InvocationTargetException;
import java.lang.reflect.Method;
import java.lang.reflect.Modifier;
import java.net.URL;
import java.net.URLClassLoader;
import java.nio.file.Path;
import java.nio.file.Paths;
import java.util.Arrays;
import java.util.jar.JarFile;
import java.util.jar.Manifest;

/** 只调度两个既有工具的公开主类；不接受类名、虚拟机参数或额外类路径。 */
final class JarLauncher {
    static boolean supports(String[] request) {
        return request.length > 0
            && (request[0].equals("apksigner") || request[0].equals("apktool"));
    }

    static int run(String[] request) {
        if (request.length < 3) {
            return reject("JAVA_ADAPTER_REQUEST_INVALID", 2);
        }
        for (String value : request) {
            if (value.indexOf('\0') >= 0) {
                return reject("JAVA_ADAPTER_REQUEST_INVALID", 2);
            }
        }
        String entry = request[0].equals("apksigner")
            ? "com.android.apksigner.ApkSignerTool" : "brut.apktool.Main";
        ClassLoader previous = Thread.currentThread().getContextClassLoader();
        try {
            Path path = Paths.get(request[1]);
            if (!path.isAbsolute()) {
                return reject("JAVA_ADAPTER_JAR_INVALID", 3);
            }
            try (JarFile jar = new JarFile(path.toFile())) {
                Manifest manifest = jar.getManifest();
                if (manifest == null || !entry.equals(
                        manifest.getMainAttributes().getValue("Main-Class"))) {
                    return reject("JAVA_ADAPTER_JAR_INVALID", 3);
                }
            }
            // 工具可能在主函数返回后由非守护线程读取资源，加载器归整个 JVM 生命周期。
            // 不在主函数返回处 close；退出仍由原工具或外层进程监督负责。
            URLClassLoader loader = new URLClassLoader(
                new URL[] {path.toUri().toURL()}, JarLauncher.class.getClassLoader());
            Thread.currentThread().setContextClassLoader(loader);
            Class<?> tool = Class.forName(entry, true, loader);
            Method main = tool.getMethod("main", String[].class);
            if (tool.getClassLoader() != loader || main.getReturnType() != Void.TYPE
                    || !Modifier.isStatic(main.getModifiers())) {
                return reject("JAVA_ADAPTER_JAR_INVALID", 3);
            }
            // 明确两个输出流的字节编码；输出内容不包装成证书 JSON，由核心分别限量捕获。
            System.setOut(new PrintStream(System.out, true, "UTF-8"));
            System.setErr(new PrintStream(System.err, true, "UTF-8"));
            main.invoke(null, (Object) Arrays.copyOfRange(request, 2, request.length));
            return 0;
        } catch (InvocationTargetException error) {
            // 不回显工具异常原文；原工具主动 System.exit 的退出码不在此拦截。
            return reject("JAVA_ADAPTER_TOOL_EXCEPTION", 1);
        } catch (Exception error) {
            return reject("JAVA_ADAPTER_JAR_INVALID", 3);
        } catch (LinkageError error) {
            return reject("JAVA_ADAPTER_JAR_INVALID", 3);
        } finally {
            Thread.currentThread().setContextClassLoader(previous);
        }
    }

    private static int reject(String code, int status) {
        System.err.println(code);
        return status;
    }

    private JarLauncher() {}
}

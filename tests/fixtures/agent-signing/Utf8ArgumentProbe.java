import java.io.DataInputStream;
import java.io.IOException;
import java.net.URL;
import java.net.URLClassLoader;
import java.nio.ByteBuffer;
import java.nio.charset.CodingErrorAction;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Paths;
import java.util.Arrays;
import java.util.jar.JarFile;

// 仅用于隔离可行性测试，不是正式工具启动器或公共协议。
public final class Utf8ArgumentProbe {
    public static void main(String[] ignored) {
        final String[] request;
        try {
            request = readFrame();
            if (request.length == 0) {
                throw new IOException();
            }
        } catch (Exception error) {
            System.err.println("自有 UTF-8 原型拒绝输入");
            System.exit(2);
            return;
        }
        try {
            String[] args = Arrays.copyOfRange(request, 1, request.length);
            if (request[0].equals("roundtrip")) {
                roundtrip(args);
            } else if (request[0].equals("certificate")) {
                Utf8CertificateProbe.run(args);
            } else if (request[0].equals("apksigner")) {
                jar(args, "com.android.apksigner.ApkSignerTool");
            } else if (request[0].equals("apktool")) {
                jar(args, "brut.apktool.Main");
            } else {
                System.err.println("自有 UTF-8 原型拒绝操作");
                System.exit(2);
            }
        } catch (Throwable error) {
            // 不回传可能包含目录、Alias 或密码的工具异常原文。
            System.err.println("自有 UTF-8 原型操作失败");
            System.exit(3);
        }
    }

    private static String[] readFrame() throws IOException {
        DataInputStream input = new DataInputStream(System.in);
        if (input.readInt() != 0x55544638 || input.readInt() != 1) {
            throw new IOException();
        }
        int count = input.readInt();
        if (count < 1 || count > 64) {
            throw new IOException();
        }
        String[] args = new String[count];
        int remaining = 65536 - 12;
        for (int i = 0; i < count; i++) {
            int length = input.readInt();
            remaining -= 4;
            if (length < 0 || length > remaining) {
                throw new IOException();
            }
            byte[] bytes = new byte[length];
            input.readFully(bytes);
            args[i] = StandardCharsets.UTF_8.newDecoder()
                .onMalformedInput(CodingErrorAction.REPORT)
                .onUnmappableCharacter(CodingErrorAction.REPORT)
                .decode(ByteBuffer.wrap(bytes)).toString();
            remaining -= length;
        }
        if (input.read() != -1) {
            throw new IOException();
        }
        return args;
    }

    private static void roundtrip(String[] args) throws Exception {
        String[] expected = {"中文Release", "CN=中文主题", "空 格&引号\"反斜杠\\", "", "扩展😀"};
        if (args.length != expected.length + 1) {
            throw new IOException();
        }
        boolean matches = Arrays.equals(expected, Arrays.copyOf(args, expected.length));
        String file = new String(Files.readAllBytes(Paths.get(args[5])), StandardCharsets.UTF_8);
        System.out.println("{\"arguments_match\":" + matches
            + ",\"unicode_file_read\":" + file.equals("路径内容一致") + "}");
    }

    private static void jar(String[] args, String entry) throws Exception {
        if (args.length < 2) {
            throw new IOException();
        }
        // 入口白名单固定；不接受任意类名、shell 命令或额外模块开放选项。
        try (JarFile jar = new JarFile(args[0])) {
            if (jar.getManifest() == null || !entry.equals(
                    jar.getManifest().getMainAttributes().getValue("Main-Class"))) {
                throw new IOException();
            }
        }
        ClassLoader previous = Thread.currentThread().getContextClassLoader();
        try (URLClassLoader loader = new URLClassLoader(
                new URL[] {Paths.get(args[0]).toUri().toURL()}, Utf8ArgumentProbe.class.getClassLoader())) {
            Thread.currentThread().setContextClassLoader(loader);
            try {
                Class.forName(entry, true, loader).getMethod("main", String[].class)
                    .invoke(null, (Object) Arrays.copyOfRange(args, 1, args.length));
            } finally {
                Thread.currentThread().setContextClassLoader(previous);
            }
        }
    }
}

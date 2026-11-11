package dev.mocika.shield.tools;

import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.Paths;
import java.nio.file.StandardOpenOption;

// 只在测试 JAR 中出现；复用正式解码器，验证真实 cwd 和文件落点，不打印路径。
public final class BootstrapProbe {
    public static void main(String[] ignored) throws Exception {
        String[] args = Request.read(System.in);
        if (args.length != 3 || !args[0].equals("bootstrap")) {
            throw new IllegalArgumentException();
        }
        boolean cwd = false;
        boolean read = false;
        boolean write = false;
        boolean property = false;
        try {
            cwd = Files.isSameFile(Paths.get("."), Paths.get(args[1]));
            read = new String(Files.readAllBytes(Paths.get(args[2])), StandardCharsets.UTF_8)
                .equals("引导读取一致");
            Path actual = Paths.get("landed.txt");
            Files.write(actual, "引导输出一致".getBytes(StandardCharsets.UTF_8),
                StandardOpenOption.CREATE_NEW, StandardOpenOption.WRITE);
            write = Files.isSameFile(actual, Paths.get(args[1]).resolve("landed.txt"));
            property = Files.isSameFile(Paths.get(System.getProperty("user.dir")), Paths.get(args[1]));
        } catch (java.io.IOException error) {
            // 有限布尔诊断；任一失败由 Rust 断言拒绝，不回显原始目录。
        }
        System.out.println("{\"cwd_matches\":" + cwd + ",\"input_matches\":" + read
            + ",\"output_matches\":" + write + ",\"user_dir_matches\":" + property + "}");
    }
}

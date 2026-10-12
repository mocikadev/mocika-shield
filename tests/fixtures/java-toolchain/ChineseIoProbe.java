import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Paths;

/** 验证路径参数与文本输出，不参与正式打包。 */
public final class ChineseIoProbe {
    public static void main(String[] args) throws Exception {
        String content = new String(Files.readAllBytes(Paths.get(args[0])), StandardCharsets.UTF_8);
        System.out.println(args[0]);
        System.out.println(content);
        System.err.println("中文错误详情：" + args[0]);
    }
}

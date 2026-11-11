import java.io.File;
import java.io.IOException;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Paths;

// 仅返回参数是否原样到达和规范化结果，不输出参数原文或目录。
public final class ToolPathProbe {
    public static void main(String[] args) throws Exception {
        String expected = new String(Files.readAllBytes(Paths.get(args[1])), StandardCharsets.UTF_8);
        boolean canonical = true;
        try {
            new File(args[0]).getCanonicalPath();
        } catch (IOException e) {
            canonical = false;
        }
        System.out.println("{\"argument_matches\":" + expected.equals(args[0])
            + ",\"has_question_mark\":" + (args[0].indexOf('?') >= 0)
            + ",\"has_replacement\":" + (args[0].indexOf('\ufffd') >= 0)
            + ",\"canonicalized\":" + canonical + "}");
    }
}

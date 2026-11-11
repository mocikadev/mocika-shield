import java.io.DataOutputStream;
import java.io.File;
import java.nio.charset.StandardCharsets;

/** 自有测试探针：用定长 UTF-8 帧核对属性，不依赖终端或日志编码。 */
public final class RuntimeProperties {
    public static void main(String[] args) throws Exception {
        DataOutputStream output = new DataOutputStream(System.out);
        // 只在本次隔离目录原子新建两个标记，验证实际目录落点，不覆盖已有文件。
        if (!new File(System.getProperty("user.home"), "home.marker").createNewFile()
                || !new File(System.getProperty("java.io.tmpdir"), "temp.marker").createNewFile()) {
            throw new IllegalStateException("测试目录已占用");
        }
        for (String name : new String[] {"user.home", "java.io.tmpdir", "java.version"}) {
            byte[] value = System.getProperty(name).getBytes(StandardCharsets.UTF_8);
            output.writeInt(value.length);
            output.write(value);
        }
        output.flush();
    }
}

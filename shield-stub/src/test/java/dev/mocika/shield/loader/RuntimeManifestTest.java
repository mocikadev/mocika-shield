package dev.mocika.shield.loader;

import org.junit.Test;

import static org.junit.Assert.assertEquals;
import static org.junit.Assert.fail;

public class RuntimeManifestTest {
    @Test
    public void 原应用字符串必须存在且非空() {
        assertEquals("android.app.Application", RuntimeManifest.requiredText("android.app.Application"));
        for (Object value : new Object[] {null, "", " ", 1, true}) {
            try {
                RuntimeManifest.requiredText(value);
                fail("无效元数据不应被接受");
            } catch (SecurityException error) {
                assertEquals("T02", error.getMessage());
            }
        }
    }

    @Test
    public void 缺失未知和错误类型策略不能降级为兼容() {
        assertEquals(false, RuntimeSecurity.parsePolicy("compatible"));
        assertEquals(true, RuntimeSecurity.parsePolicy("strict"));
        for (Object value : new Object[] {null, "", "unknown", "Strict", "strict ", 1, true}) {
            try {
                RuntimeSecurity.parsePolicy(value);
                fail("无效策略不应降级");
            } catch (SecurityException error) {
                assertEquals("S02", error.getMessage());
            }
        }
    }
}

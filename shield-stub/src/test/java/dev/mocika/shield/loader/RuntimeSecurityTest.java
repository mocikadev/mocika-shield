package dev.mocika.shield.loader;

import org.junit.Test;

import static org.junit.Assert.assertEquals;
import static org.junit.Assert.fail;

public class RuntimeSecurityTest {

    @Test
    public void 安全环境允许继续启动() {
        RuntimeSecurity.enforceSafe(false);
    }

    @Test
    public void 不安全环境拒绝继续启动() {
        try {
            RuntimeSecurity.enforceSafe(true);
            fail("不安全环境未被拒绝");
        } catch (SecurityException error) {
            assertEquals("S01", error.getMessage());
        }
    }

    @Test
    public void 兼容模式忽略Root但始终拒绝反调试() {
        assertEquals(false, RuntimeSecurity.shouldReject(2, false));
        assertEquals(true, RuntimeSecurity.shouldReject(1, false));
    }

    @Test
    public void 严格模式拒绝Root信号() {
        assertEquals(true, RuntimeSecurity.shouldReject(2, true));
    }
}

package dev.mocika.shield.loader;

import org.junit.Test;

public class RuntimeSecurityTest {

    @Test
    public void 安全环境允许继续启动() {
        RuntimeSecurity.enforceSafe(false);
    }

    @Test(expected = SecurityException.class)
    public void 不安全环境拒绝继续启动() {
        RuntimeSecurity.enforceSafe(true);
    }
}

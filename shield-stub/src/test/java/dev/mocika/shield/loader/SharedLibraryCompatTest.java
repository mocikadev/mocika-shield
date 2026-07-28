package dev.mocika.shield.loader;

import org.junit.Test;

import static org.junit.Assert.assertFalse;
import static org.junit.Assert.assertTrue;

public class SharedLibraryCompatTest {

    @Test
    public void 仅Android9且声明Apache共享库时启用() {
        String[] libraries = new String[]{"/system/framework/org.apache.http.legacy.boot.jar"};

        assertFalse(SharedLibraryCompat.shouldPrepare(27, libraries));
        assertTrue(SharedLibraryCompat.shouldPrepare(28, libraries));
        assertFalse(SharedLibraryCompat.shouldPrepare(29, libraries));
    }

    @Test
    public void 没有共享库时跳过() {
        assertFalse(SharedLibraryCompat.shouldPrepare(28, null));
        assertFalse(SharedLibraryCompat.shouldPrepare(28, new String[0]));
        assertFalse(SharedLibraryCompat.shouldPrepare(
                28, new String[]{"/system/framework/其他库.jar"}));
    }

    @Test
    public void 仅预解析Apache共享库包名() {
        assertTrue(SharedLibraryCompat.isLegacyHttpClass(
                "org.apache.http.message.AbstractHttpMessage"));
        assertTrue(SharedLibraryCompat.isLegacyHttpClass("android.net.http.Headers"));
        assertFalse(SharedLibraryCompat.isLegacyHttpClass("com.example.HttpClient"));
    }
}

package dev.mocika.shield.loader;

import org.junit.Test;

import static org.junit.Assert.assertArrayEquals;

public class DexInjectorTest {

    @Test
    public void api21和22优先使用makeDexElements() {
        assertArrayEquals(
                new String[]{"makeDexElements", "makePathElements"},
                DexInjector.factoryMethodNames(21));
        assertArrayEquals(
                new String[]{"makeDexElements", "makePathElements"},
                DexInjector.factoryMethodNames(22));
    }

    @Test
    public void api23优先使用makePathElements() {
        assertArrayEquals(
                new String[]{"makePathElements", "makeDexElements"},
                DexInjector.factoryMethodNames(23));
    }

    @Test
    public void 解密dex元素插入原元素之前() {
        String[] original = new String[]{"壳"};
        String[] injected = new String[]{"业务1", "业务2"};

        assertArrayEquals(
                new String[]{"业务1", "业务2", "壳"},
                DexInjector.prepend(original, injected));
    }
}

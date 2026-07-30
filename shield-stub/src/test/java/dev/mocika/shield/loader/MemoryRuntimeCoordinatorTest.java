package dev.mocika.shield.loader;

import org.junit.Test;

import static org.junit.Assert.assertFalse;
import static org.junit.Assert.assertTrue;

public class MemoryRuntimeCoordinatorTest {
    @Test
    public void android十二以下固定使用文件路径() {
        assertFalse(MemoryRuntimeCoordinator.usesMemory(28));
        assertFalse(MemoryRuntimeCoordinator.usesMemory(29));
        assertFalse(MemoryRuntimeCoordinator.usesMemory(30));
    }

    @Test
    public void android十二起启用内存路径() {
        assertTrue(MemoryRuntimeCoordinator.usesMemory(31));
        assertTrue(MemoryRuntimeCoordinator.usesMemory(35));
    }
}

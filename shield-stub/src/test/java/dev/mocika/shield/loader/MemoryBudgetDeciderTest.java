package dev.mocika.shield.loader;

import org.junit.Test;

import static org.junit.Assert.assertEquals;
import static org.junit.Assert.assertFalse;
import static org.junit.Assert.assertTrue;

public class MemoryBudgetDeciderTest {
    private static final long MIB = 1024L * 1024L;
    private static final long GIB = 1024L * MIB;

    @Test
    public void 高内存六十四位设备允许中等载荷() {
        MemoryBudgetDecider.Decision decision = decide(35, 8 * GIB, 3 * GIB,
                false, true, 113 * MIB);
        assertTrue(decision.allowed);
        assertEquals(MemoryBudgetDecider.Reason.ALLOWED, decision.reason);
    }

    @Test
    public void 低内存设备始终使用文件路径() {
        MemoryBudgetDecider.Decision decision = decide(35, 8 * GIB, 3 * GIB,
                true, true, 20 * MIB);
        assertFalse(decision.allowed);
        assertEquals(MemoryBudgetDecider.Reason.LOW_RAM_DEVICE, decision.reason);
    }

    @Test
    public void 可用内存不足时拒绝内存路径() {
        MemoryBudgetDecider.Decision decision = decide(35, 8 * GIB, 600 * MIB,
                false, true, 113 * MIB);
        assertFalse(decision.allowed);
        assertEquals(MemoryBudgetDecider.Reason.AVAILABLE_MEMORY_LOW, decision.reason);
    }

    @Test
    public void 物理内存不足时拒绝内存路径() {
        MemoryBudgetDecider.Decision decision = decide(35, 2 * GIB, 1500 * MIB,
                false, true, 20 * MIB);
        assertFalse(decision.allowed);
        assertEquals(MemoryBudgetDecider.Reason.TOTAL_MEMORY_LOW, decision.reason);
    }

    @Test
    public void 三十二位进程使用更严格的载荷上限() {
        MemoryBudgetDecider.Decision decision = decide(35, 4 * GIB, 2 * GIB,
                false, false, 65 * MIB);
        assertFalse(decision.allowed);
        assertEquals(MemoryBudgetDecider.Reason.PAYLOAD_TOO_LARGE, decision.reason);
    }

    @Test
    public void 元数据缺失或系统过低时安全降级() {
        assertEquals(MemoryBudgetDecider.Reason.INVALID_METRICS,
                decide(35, 8 * GIB, 3 * GIB, false, true, 0).reason);
        assertEquals(MemoryBudgetDecider.Reason.API_TOO_LOW,
                decide(30, 8 * GIB, 3 * GIB, false, true, 20 * MIB).reason);
    }

    @Test
    public void 六十四位超大载荷也不会绕过绝对上限() {
        MemoryBudgetDecider.Decision decision = decide(35, 16 * GIB, 8 * GIB,
                false, true, 385 * MIB);
        assertFalse(decision.allowed);
        assertEquals(MemoryBudgetDecider.Reason.PAYLOAD_TOO_LARGE, decision.reason);
    }

    private static MemoryBudgetDecider.Decision decide(int api, long total, long available,
            boolean lowRam, boolean process64Bit, long payload) {
        return MemoryBudgetDecider.decide(new MemoryBudgetSnapshot(
                api, total, available, lowRam, process64Bit, payload));
    }
}

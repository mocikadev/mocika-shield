package dev.mocika.shield.loader;

import org.junit.Test;

import static org.junit.Assert.assertEquals;
import static org.junit.Assert.fail;

public class RecoveryStateMachineTest {
    private static final String IDENTITY = repeat('a', 64);

    @Test
    public void 新载荷按本次预算选择路径() {
        assertEquals(RecoveryStateMachine.Plan.MEMORY,
                RecoveryStateMachine.begin(IDENTITY, null, true));
        assertEquals(RecoveryStateMachine.Plan.FILE_BUDGET,
                RecoveryStateMachine.begin(IDENTITY, null, false));
    }

    @Test
    public void 预算文件成功不会形成粘性回退() {
        RecoveryStateMachine.Previous previous = record(
                IDENTITY, RecoveryStateMachine.FILE_READY);
        assertEquals(RecoveryStateMachine.Plan.MEMORY,
                RecoveryStateMachine.begin(IDENTITY, previous, true));
        assertEquals(RecoveryStateMachine.Plan.FILE_BUDGET,
                RecoveryStateMachine.begin(IDENTITY, previous, false));
    }

    @Test
    public void 未确认的内存启动进入粘性文件回退() {
        assertEquals(RecoveryStateMachine.Plan.FILE_RECOVERY,
                RecoveryStateMachine.begin(IDENTITY,
                        record(IDENTITY, RecoveryStateMachine.MEMORY_PENDING), true));
        assertEquals(RecoveryStateMachine.Plan.FILE_RECOVERY,
                RecoveryStateMachine.begin(IDENTITY,
                        record(IDENTITY, RecoveryStateMachine.FILE_FALLBACK), true));
    }

    @Test
    public void 未确认的文件启动失败关闭() {
        try {
            RecoveryStateMachine.begin(IDENTITY,
                    record(IDENTITY, RecoveryStateMachine.FILE_PENDING), true);
            fail("文件路径失败后不应继续启动");
        } catch (SecurityException expected) {
            assertEquals("R01", expected.getMessage());
        }
    }

    @Test
    public void 完成状态区分预算文件与崩溃回退() {
        assertEquals(RecoveryStateMachine.MEMORY_READY,
                RecoveryStateMachine.complete(RecoveryStateMachine.Plan.MEMORY));
        assertEquals(RecoveryStateMachine.FILE_READY,
                RecoveryStateMachine.complete(RecoveryStateMachine.Plan.FILE_BUDGET));
        assertEquals(RecoveryStateMachine.FILE_FALLBACK,
                RecoveryStateMachine.complete(RecoveryStateMachine.Plan.FILE_RECOVERY));
    }

    private static RecoveryStateMachine.Previous record(String identity, String state) {
        return new RecoveryStateMachine.Previous(identity, state);
    }

    private static String repeat(char value, int count) {
        StringBuilder result = new StringBuilder(count);
        for (int index = 0; index < count; index++) result.append(value);
        return result.toString();
    }
}

package dev.mocika.shield.loader;

import org.junit.Test;

import static org.junit.Assert.assertEquals;
import static org.junit.Assert.fail;

public class RecoveryStateMachineTest {
    private static final String IDENTITY = repeat('a', 64);

    @Test
    public void 新载荷优先选择内存模式() {
        assertEquals(RecoveryStateMachine.Mode.MEMORY,
                RecoveryStateMachine.begin(IDENTITY, null));
        assertEquals(RecoveryStateMachine.Mode.MEMORY,
                RecoveryStateMachine.begin(IDENTITY,
                        record(repeat('b', 64), RecoveryStateMachine.FILE_FALLBACK)));
    }

    @Test
    public void 未确认的内存启动进入粘性文件回退() {
        assertEquals(RecoveryStateMachine.Mode.FILE,
                RecoveryStateMachine.begin(IDENTITY,
                        record(IDENTITY, RecoveryStateMachine.MEMORY_PENDING)));
        assertEquals(RecoveryStateMachine.Mode.FILE,
                RecoveryStateMachine.begin(IDENTITY,
                        record(IDENTITY, RecoveryStateMachine.FILE_FALLBACK)));
    }

    @Test
    public void 未确认的文件启动失败关闭() {
        try {
            RecoveryStateMachine.begin(IDENTITY,
                    record(IDENTITY, RecoveryStateMachine.FILE_PENDING));
            fail("文件回退失败后不应继续启动");
        } catch (SecurityException expected) {
            assertEquals("R01", expected.getMessage());
        }
    }

    @Test
    public void 完成状态与模式严格对应() {
        assertEquals(RecoveryStateMachine.MEMORY_PENDING,
                RecoveryStateMachine.pending(RecoveryStateMachine.Mode.MEMORY));
        assertEquals(RecoveryStateMachine.MEMORY_READY,
                RecoveryStateMachine.complete(RecoveryStateMachine.Mode.MEMORY));
        assertEquals(RecoveryStateMachine.FILE_PENDING,
                RecoveryStateMachine.pending(RecoveryStateMachine.Mode.FILE));
        assertEquals(RecoveryStateMachine.FILE_FALLBACK,
                RecoveryStateMachine.complete(RecoveryStateMachine.Mode.FILE));
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

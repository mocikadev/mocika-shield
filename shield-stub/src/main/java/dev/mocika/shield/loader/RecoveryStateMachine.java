package dev.mocika.shield.loader;

/** 定义内存启动与文件回退的合法状态转换，不负责持久化。 */
final class RecoveryStateMachine {
    enum Mode { MEMORY, FILE }

    static final String MEMORY_READY = "memory_ready";
    static final String MEMORY_PENDING = "memory_pending";
    static final String FILE_PENDING = "file_pending";
    static final String FILE_FALLBACK = "file_fallback";

    private RecoveryStateMachine() {}

    static Mode begin(String identity, Previous previous) {
        if (previous == null || !identity.equals(previous.identity)) return Mode.MEMORY;
        if (MEMORY_READY.equals(previous.state)) return Mode.MEMORY;
        if (MEMORY_PENDING.equals(previous.state)
                || FILE_FALLBACK.equals(previous.state)) return Mode.FILE;
        if (FILE_PENDING.equals(previous.state)) throw new SecurityException("R01");
        throw new SecurityException("R02");
    }

    static String pending(Mode mode) {
        return mode == Mode.MEMORY ? MEMORY_PENDING : FILE_PENDING;
    }

    static String complete(Mode mode) {
        return mode == Mode.MEMORY ? MEMORY_READY : FILE_FALLBACK;
    }

    static final class Previous {
        final String identity;
        final String state;

        Previous(String identity, String state) {
            this.identity = identity;
            this.state = state;
        }
    }
}

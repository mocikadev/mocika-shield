package dev.mocika.shield.loader;

/** 定义内存启动与文件回退的合法状态转换，不负责持久化。 */
final class RecoveryStateMachine {
    enum Mode { MEMORY, FILE }
    enum Plan { MEMORY, FILE_BUDGET, FILE_RECOVERY }

    static final String MEMORY_READY = "memory_ready";
    static final String MEMORY_PENDING = "memory_pending";
    static final String FILE_PENDING = "file_pending";
    static final String FILE_FALLBACK = "file_fallback";
    static final String FILE_READY = "file_ready";

    private RecoveryStateMachine() {}

    static Plan begin(String identity, Previous previous, boolean memoryAllowed) {
        if (previous == null || !identity.equals(previous.identity)) {
            return memoryAllowed ? Plan.MEMORY : Plan.FILE_BUDGET;
        }
        if (MEMORY_READY.equals(previous.state) || FILE_READY.equals(previous.state)) {
            return memoryAllowed ? Plan.MEMORY : Plan.FILE_BUDGET;
        }
        if (MEMORY_PENDING.equals(previous.state)
                || FILE_FALLBACK.equals(previous.state)) return Plan.FILE_RECOVERY;
        if (FILE_PENDING.equals(previous.state)) throw new SecurityException("R01");
        throw new SecurityException("R02");
    }

    static Mode mode(Plan plan) {
        return plan == Plan.MEMORY ? Mode.MEMORY : Mode.FILE;
    }

    static String pending(Mode mode) {
        return mode == Mode.MEMORY ? MEMORY_PENDING : FILE_PENDING;
    }

    static String complete(Plan plan) {
        if (plan == Plan.MEMORY) return MEMORY_READY;
        return plan == Plan.FILE_BUDGET ? FILE_READY : FILE_FALLBACK;
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

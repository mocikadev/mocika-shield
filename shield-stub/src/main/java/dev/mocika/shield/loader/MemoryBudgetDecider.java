package dev.mocika.shield.loader;

/** 依据不可变设备快照判断本次进程是否具备内存加载预算。 */
final class MemoryBudgetDecider {
    private static final long MIB = 1024L * 1024L;
    private static final long GIB = 1024L * MIB;
    private static final long MAX_PAYLOAD_64 = 384L * MIB;
    private static final long MAX_PAYLOAD_32 = 64L * MIB;

    enum Reason {
        ALLOWED,
        API_TOO_LOW,
        INVALID_METRICS,
        LOW_RAM_DEVICE,
        PAYLOAD_TOO_LARGE,
        TOTAL_MEMORY_LOW,
        AVAILABLE_MEMORY_LOW
    }

    static final class Decision {
        final boolean allowed;
        final Reason reason;

        Decision(boolean allowed, Reason reason) {
            this.allowed = allowed;
            this.reason = reason;
        }
    }

    private MemoryBudgetDecider() {}

    static Decision decide(MemoryBudgetSnapshot snapshot) {
        if (snapshot.sdkInt < 31) return denied(Reason.API_TOO_LOW);
        if (snapshot.payloadDexBytes <= 0 || snapshot.totalMemoryBytes <= 0
                || snapshot.availableMemoryBytes <= 0) {
            return denied(Reason.INVALID_METRICS);
        }
        if (snapshot.lowRamDevice) return denied(Reason.LOW_RAM_DEVICE);

        long payloadLimit = snapshot.process64Bit ? MAX_PAYLOAD_64 : MAX_PAYLOAD_32;
        if (snapshot.payloadDexBytes > payloadLimit) return denied(Reason.PAYLOAD_TOO_LARGE);

        long totalFloor = 3L * GIB;
        long totalMultiplier = snapshot.process64Bit ? 10L : 12L;
        long requiredTotal = Math.max(totalFloor,
                saturatedMultiply(snapshot.payloadDexBytes, totalMultiplier));
        if (snapshot.totalMemoryBytes < requiredTotal) return denied(Reason.TOTAL_MEMORY_LOW);

        long availableFloor = snapshot.process64Bit ? 768L * MIB : GIB;
        long availableMultiplier = snapshot.process64Bit ? 5L : 7L;
        long requiredAvailable = Math.max(availableFloor,
                saturatedMultiply(snapshot.payloadDexBytes, availableMultiplier));
        if (snapshot.availableMemoryBytes < requiredAvailable) {
            return denied(Reason.AVAILABLE_MEMORY_LOW);
        }
        return new Decision(true, Reason.ALLOWED);
    }

    private static Decision denied(Reason reason) {
        return new Decision(false, reason);
    }

    private static long saturatedMultiply(long value, long multiplier) {
        if (value > Long.MAX_VALUE / multiplier) return Long.MAX_VALUE;
        return value * multiplier;
    }
}

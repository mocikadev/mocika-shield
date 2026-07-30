package dev.mocika.shield.loader;

/** 内存路径决策所需的不可变运行时快照，不依赖具体采集方式。 */
final class MemoryBudgetSnapshot {
    final int sdkInt;
    final long totalMemoryBytes;
    final long availableMemoryBytes;
    final boolean lowRamDevice;
    final boolean process64Bit;
    final long payloadDexBytes;

    MemoryBudgetSnapshot(int sdkInt, long totalMemoryBytes, long availableMemoryBytes,
            boolean lowRamDevice, boolean process64Bit, long payloadDexBytes) {
        this.sdkInt = sdkInt;
        this.totalMemoryBytes = totalMemoryBytes;
        this.availableMemoryBytes = availableMemoryBytes;
        this.lowRamDevice = lowRamDevice;
        this.process64Bit = process64Bit;
        this.payloadDexBytes = payloadDexBytes;
    }
}

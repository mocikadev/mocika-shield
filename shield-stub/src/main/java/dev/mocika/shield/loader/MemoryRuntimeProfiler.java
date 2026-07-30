package dev.mocika.shield.loader;

import android.os.SystemClock;
import android.util.Log;

import dev.mocika.shield.stub.BuildConfig;

/** 仅供内存候选资源记录启动阶段，不持久化状态或参与运行决策。 */
final class MemoryRuntimeProfiler {
    private static final String TAG = "mxp";

    private final long startedAt;
    private long previousAt;

    private MemoryRuntimeProfiler() {
        startedAt = SystemClock.elapsedRealtimeNanos();
        previousAt = startedAt;
    }

    static MemoryRuntimeProfiler start() {
        if (!BuildConfig.RUNTIME_PROFILE) return null;
        MemoryRuntimeProfiler profiler = new MemoryRuntimeProfiler();
        profiler.stage("begin", 0, 0);
        return profiler;
    }

    void stage(String name, int dexCount, long dexBytes) {
        long now = SystemClock.elapsedRealtimeNanos();
        long stepMillis = nanosToMillis(now - previousAt);
        long totalMillis = nanosToMillis(now - startedAt);
        Runtime runtime = Runtime.getRuntime();
        long heapUsedKb = (runtime.totalMemory() - runtime.freeMemory()) / 1024;
        Log.i(TAG, "stage=" + name
                + " step_ms=" + stepMillis
                + " total_ms=" + totalMillis
                + " dex_count=" + dexCount
                + " dex_bytes=" + dexBytes
                + " heap_used_kb=" + heapUsedKb);
        previousAt = now;
    }

    private static long nanosToMillis(long nanos) {
        return nanos / 1_000_000L;
    }
}

package dev.mocika.shield.memoryprobe;

import android.content.Context;
import android.content.pm.ApplicationInfo;
import android.content.pm.PackageManager;
import android.os.Debug;
import android.os.Handler;
import android.os.Looper;
import android.util.Log;

import java.lang.ref.WeakReference;
import java.nio.ByteBuffer;

/** 仅为隔离探针记录载荷对象生命周期和进程内存，不进入正式壳。 */
final class MemoryProbeMetrics {
    private static final String TAG = "MOCIKA_MEMORY_METRICS";
    private static WeakReference<byte[]> encryptedPayload;
    private static WeakReference<byte[][]> decryptedPayload;
    private static WeakReference<byte[]> firstDex;
    private static WeakReference<ByteBuffer[]> directBuffers;
    private static WeakReference<ByteBuffer> firstDirectBuffer;
    private static boolean enabled;

    private MemoryProbeMetrics() {
    }

    static void configure(Context context) throws Exception {
        ApplicationInfo info = context.getPackageManager().getApplicationInfo(
                context.getPackageName(), PackageManager.GET_META_DATA);
        enabled = info.metaData != null
                && info.metaData.getBoolean("PROBE_MEMORY_METRICS", false);
    }

    static void trackEncrypted(byte[] payload) {
        if (!enabled) return;
        encryptedPayload = new WeakReference<>(payload);
    }

    static void trackDecrypted(byte[][] payload) {
        if (!enabled) return;
        decryptedPayload = new WeakReference<>(payload);
        firstDex = new WeakReference<>(payload.length == 0 ? null : payload[0]);
    }

    static void trackDirectBuffers(ByteBuffer[] payload) {
        if (!enabled) return;
        directBuffers = new WeakReference<>(payload);
        firstDirectBuffer = new WeakReference<>(payload.length == 0 ? null : payload[0]);
    }

    static void snapshot(String phase) {
        if (!enabled) return;
        Debug.MemoryInfo info = new Debug.MemoryInfo();
        Debug.getMemoryInfo(info);
        Runtime runtime = Runtime.getRuntime();
        long javaHeapKb = (runtime.totalMemory() - runtime.freeMemory()) / 1024L;
        Log.i(TAG, "SNAPSHOT:" + phase
                + ":total_pss_kb=" + info.getTotalPss()
                + ":dalvik_pss_kb=" + info.dalvikPss
                + ":native_pss_kb=" + info.nativePss
                + ":other_pss_kb=" + info.otherPss
                + ":java_heap_kb=" + javaHeapKb
                + ":encrypted_alive=" + alive(encryptedPayload)
                + ":decrypted_outer_alive=" + alive(decryptedPayload)
                + ":first_dex_alive=" + alive(firstDex)
                + ":direct_array_alive=" + alive(directBuffers)
                + ":first_direct_alive=" + alive(firstDirectBuffer));
    }

    static void schedulePostStartupSnapshot() {
        if (!enabled) return;
        new Handler(Looper.getMainLooper()).postDelayed(() -> {
            System.gc();
            System.runFinalization();
            new Handler(Looper.getMainLooper()).postDelayed(
                    () -> snapshot("post_startup_gc"), 500L);
        }, 1_500L);
    }

    private static boolean alive(WeakReference<?> reference) {
        return reference != null && reference.get() != null;
    }
}

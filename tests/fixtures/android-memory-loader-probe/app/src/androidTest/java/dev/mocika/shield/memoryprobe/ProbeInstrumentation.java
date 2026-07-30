package dev.mocika.shield.memoryprobe;

import android.app.Instrumentation;
import android.content.Context;
import android.content.Intent;
import android.os.Bundle;
import android.util.Log;

/** 验证外部测试 APK 的 Instrumentation 能观察并使用业务代理加载器。 */
public final class ProbeInstrumentation extends Instrumentation {
    private static final String TAG = "MOCIKA_MEMORY_PROBE";

    @Override
    public void onCreate(Bundle arguments) {
        super.onCreate(arguments);
        start();
    }

    @Override
    public void onStart() {
        try {
            Context target = getTargetContext();
            ClassLoader loader = target.getClassLoader();
            Class<?> activity = loader.loadClass(
                    "dev.mocika.shield.memorypayload.PayloadActivity");
            if (!loader.getClass().getName().endsWith("DeferredPayloadClassLoader")) {
                throw new IllegalStateException("INSTRUMENTATION_TARGET_LOADER_STALE");
            }
            if (activity.getClassLoader() == getClass().getClassLoader()) {
                throw new IllegalStateException("INSTRUMENTATION_TEST_LOADER_LEAKED");
            }
            Intent intent = new Intent(target, activity)
                    .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK);
            target.startActivity(intent);
            Log.i(TAG, "INSTRUMENTATION_LOADER_OK");
            finish(0, new Bundle());
        } catch (Throwable error) {
            Log.e(TAG, "INSTRUMENTATION_LOADER_FAILED", error);
            finish(1, new Bundle());
        }
    }
}

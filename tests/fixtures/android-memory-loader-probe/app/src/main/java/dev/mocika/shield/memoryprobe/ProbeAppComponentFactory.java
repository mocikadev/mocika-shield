package dev.mocika.shield.memoryprobe;

import android.app.AppComponentFactory;
import android.content.pm.ApplicationInfo;
import android.util.Log;

/** 使用系统公开入口创建探针业务加载器，不进入正式壳。 */
public final class ProbeAppComponentFactory extends AppComponentFactory {
    private static final String TAG = "MOCIKA_MEMORY_PROBE";

    @Override
    public ClassLoader instantiateClassLoader(ClassLoader defaultLoader, ApplicationInfo info) {
        try {
            ClassLoader loader = MemoryPayloadLoader.create(info, defaultLoader);
            Log.i(TAG, "FACTORY_LOADER_CREATED");
            return loader;
        } catch (Exception error) {
            throw new RuntimeException("MEMORY_PROBE_FACTORY_INIT", error);
        }
    }
}

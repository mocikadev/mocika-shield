package dev.mocika.shield.loader;

import android.app.Application;
import android.content.Context;

/** 隔离 API 28 以上类型，防止旧系统校验 StubApp 时解析组件工厂。 */
final class MemoryRuntimeBridge {
    private MemoryRuntimeBridge() {}

    static ClassLoader initialize(Context context, String originalFactoryClass) throws Exception {
        return StubComponentFactory.initialize(context, originalFactoryClass);
    }

    static Application instantiateApplication(ClassLoader loader, String className)
            throws Exception {
        return StubComponentFactory.instantiateOriginalApplication(loader, className);
    }

    static void complete() throws Exception {
        MemoryRuntimeCoordinator.complete();
    }
}

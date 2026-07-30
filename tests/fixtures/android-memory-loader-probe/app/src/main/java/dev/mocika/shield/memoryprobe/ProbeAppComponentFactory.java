package dev.mocika.shield.memoryprobe;

import android.app.AppComponentFactory;
import android.app.Activity;
import android.app.Application;
import android.app.Service;
import android.content.BroadcastReceiver;
import android.content.ContentProvider;
import android.content.Intent;
import android.content.pm.ApplicationInfo;
import android.util.Log;

/** 使用系统公开入口创建探针业务加载器，不进入正式壳。 */
public final class ProbeAppComponentFactory extends AppComponentFactory {
    private static final String TAG = "MOCIKA_MEMORY_PROBE";
    private static final String PROBE_APPLICATION =
            "dev.mocika.shield.memoryprobe.ProbeApplication";
    private static final String ORIGINAL_FACTORY =
            "dev.mocika.shield.memorypayload.PayloadAppComponentFactory";
    private static ProbeAppComponentFactory activeFactory;

    private AppComponentFactory originalFactory;

    @Override
    public ClassLoader instantiateClassLoader(ClassLoader defaultLoader, ApplicationInfo info) {
        try {
            ClassLoader loader = MemoryPayloadLoader.create(info, defaultLoader);
            originalFactory = (AppComponentFactory) loader.loadClass(ORIGINAL_FACTORY)
                    .getDeclaredConstructor().newInstance();
            activeFactory = this;
            Log.i(TAG, "FACTORY_LOADER_CREATED");
            Log.i(TAG, "FACTORY_DELEGATE_READY");
            return loader;
        } catch (Exception error) {
            throw new RuntimeException("MEMORY_PROBE_FACTORY_INIT", error);
        }
    }

    static Application instantiateOriginalApplication(ClassLoader loader, String className)
            throws Exception {
        ProbeAppComponentFactory factory = activeFactory;
        if (factory == null) {
            return (Application) loader.loadClass(className).getDeclaredConstructor().newInstance();
        }
        return factory.requireDelegate().instantiateApplication(loader, className);
    }

    @Override
    public Application instantiateApplication(ClassLoader loader, String className)
            throws InstantiationException, IllegalAccessException, ClassNotFoundException {
        if (PROBE_APPLICATION.equals(className)) {
            return super.instantiateApplication(loader, className);
        }
        return requireDelegate().instantiateApplication(loader, className);
    }

    @Override
    public Activity instantiateActivity(ClassLoader loader, String className, Intent intent)
            throws InstantiationException, IllegalAccessException, ClassNotFoundException {
        return requireDelegate().instantiateActivity(loader, className, intent);
    }

    @Override
    public Service instantiateService(ClassLoader loader, String className, Intent intent)
            throws InstantiationException, IllegalAccessException, ClassNotFoundException {
        return requireDelegate().instantiateService(loader, className, intent);
    }

    @Override
    public BroadcastReceiver instantiateReceiver(
            ClassLoader loader, String className, Intent intent)
            throws InstantiationException, IllegalAccessException, ClassNotFoundException {
        return requireDelegate().instantiateReceiver(loader, className, intent);
    }

    @Override
    public ContentProvider instantiateProvider(ClassLoader loader, String className)
            throws InstantiationException, IllegalAccessException, ClassNotFoundException {
        return requireDelegate().instantiateProvider(loader, className);
    }

    private AppComponentFactory requireDelegate() {
        if (originalFactory == null) {
            throw new IllegalStateException("MEMORY_PROBE_FACTORY_DELEGATE_MISSING");
        }
        return originalFactory;
    }
}

package dev.mocika.shield.memoryprobe;

import android.app.AppComponentFactory;
import android.app.Activity;
import android.app.Application;
import android.app.Service;
import android.content.BroadcastReceiver;
import android.content.ContentProvider;
import android.content.Intent;
import android.content.Context;
import android.content.pm.ApplicationInfo;
import android.util.Log;

/** 使用系统公开入口创建探针业务加载器，不进入正式壳。 */
public final class ProbeAppComponentFactory extends AppComponentFactory {
    private static final String TAG = "MOCIKA_MEMORY_PROBE";
    private static final String PROBE_APPLICATION =
            "dev.mocika.shield.memoryprobe.ProbeApplication";
    private static volatile FactoryState activeState;

    @Override
    public ClassLoader instantiateClassLoader(ClassLoader defaultLoader, ApplicationInfo info) {
        try {
            FactoryState state = new FactoryState(
                    new DeferredPayloadClassLoader(defaultLoader));
            activeState = state;
            verifyPayloadBlockedBeforeInitialization(state.deferredLoader);
            Log.i(TAG, "FACTORY_PROXY_CREATED");
            return state.deferredLoader;
        } catch (Exception error) {
            throw new RuntimeException("MEMORY_PROBE_FACTORY_INIT", error);
        }
    }

    static ClassLoader initializePayload(Context context, String originalFactoryClass)
            throws Exception {
        return initializePayloadOnce(requireState(), context, originalFactoryClass);
    }

    static Application instantiateOriginalApplication(ClassLoader loader, String className)
            throws Exception {
        FactoryState state = activeState;
        if (state == null) {
            return (Application) loader.loadClass(className).getDeclaredConstructor().newInstance();
        }
        return requireDelegate(state).instantiateApplication(loader, className);
    }

    @Override
    public Application instantiateApplication(ClassLoader loader, String className)
            throws InstantiationException, IllegalAccessException, ClassNotFoundException {
        if (PROBE_APPLICATION.equals(className)) {
            return super.instantiateApplication(loader, className);
        }
        return requireDelegate(requireState()).instantiateApplication(loader, className);
    }

    @Override
    public Activity instantiateActivity(ClassLoader loader, String className, Intent intent)
            throws InstantiationException, IllegalAccessException, ClassNotFoundException {
        return requireDelegate(requireState()).instantiateActivity(loader, className, intent);
    }

    @Override
    public Service instantiateService(ClassLoader loader, String className, Intent intent)
            throws InstantiationException, IllegalAccessException, ClassNotFoundException {
        return requireDelegate(requireState()).instantiateService(loader, className, intent);
    }

    @Override
    public BroadcastReceiver instantiateReceiver(
            ClassLoader loader, String className, Intent intent)
            throws InstantiationException, IllegalAccessException, ClassNotFoundException {
        return requireDelegate(requireState()).instantiateReceiver(loader, className, intent);
    }

    @Override
    public ContentProvider instantiateProvider(ClassLoader loader, String className)
            throws InstantiationException, IllegalAccessException, ClassNotFoundException {
        return requireDelegate(requireState()).instantiateProvider(loader, className);
    }

    private static AppComponentFactory requireDelegate(FactoryState state) {
        if (state.originalFactory == null) {
            throw new IllegalStateException("MEMORY_PROBE_FACTORY_DELEGATE_MISSING");
        }
        return state.originalFactory;
    }

    private static ClassLoader initializePayloadOnce(
            FactoryState state, Context context, String originalFactoryClass)
            throws Exception {
        synchronized (state) {
            if (state.originalFactory != null) {
                Log.i(TAG, "FACTORY_REINITIALIZE_STABLE");
                return state.deferredLoader;
            }
            ClassLoader candidate = MemoryPayloadLoader.create(
                    context, state.deferredLoader.getParent());
            AppComponentFactory candidateFactory = createOriginalFactory(
                    candidate, originalFactoryClass);
            state.deferredLoader.initialize(candidate);
            state.originalFactory = candidateFactory;
        }
        Log.i(TAG, "FACTORY_PAYLOAD_READY");
        Log.i(TAG, "FACTORY_DELEGATE_READY");
        return state.deferredLoader;
    }

    private static AppComponentFactory createOriginalFactory(
            ClassLoader loader, String className) throws Exception {
        if (className == null || className.trim().isEmpty()) {
            Log.i(TAG, "ORIGINAL_FACTORY_DEFAULT");
            return new AppComponentFactory();
        }
        if (ProbeAppComponentFactory.class.getName().equals(className)) {
            throw new IllegalStateException("MEMORY_PROBE_FACTORY_RECURSION");
        }
        Class<?> factoryClass = loader.loadClass(className);
        if (!AppComponentFactory.class.isAssignableFrom(factoryClass)) {
            throw new IllegalStateException("MEMORY_PROBE_FACTORY_TYPE_INVALID:" + className);
        }
        Log.i(TAG, "ORIGINAL_FACTORY_METADATA:" + className);
        return (AppComponentFactory) factoryClass.getDeclaredConstructor().newInstance();
    }

    private static void verifyPayloadBlockedBeforeInitialization(ClassLoader loader)
            throws Exception {
        try {
            loader.loadClass("dev.mocika.shield.memorypayload.PayloadAppComponentFactory");
            throw new IllegalStateException("MEMORY_PROBE_PAYLOAD_EARLY_LOAD_ALLOWED");
        } catch (ClassNotFoundException expected) {
            Log.i(TAG, "PROXY_BUSINESS_BLOCKED");
        }
    }

    private static FactoryState requireState() {
        FactoryState state = activeState;
        if (state == null) {
            throw new IllegalStateException("MEMORY_PROBE_FACTORY_MISSING");
        }
        return state;
    }

    /** 同一应用进程内所有壳工厂实例共享的加载状态。 */
    private static final class FactoryState {
        final DeferredPayloadClassLoader deferredLoader;
        volatile AppComponentFactory originalFactory;

        FactoryState(DeferredPayloadClassLoader deferredLoader) {
            this.deferredLoader = deferredLoader;
        }
    }
}

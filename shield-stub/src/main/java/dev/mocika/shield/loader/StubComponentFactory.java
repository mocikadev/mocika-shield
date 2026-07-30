package dev.mocika.shield.loader;

import android.app.Activity;
import android.app.AppComponentFactory;
import android.app.Application;
import android.app.Service;
import android.content.BroadcastReceiver;
import android.content.ContentProvider;
import android.content.Context;
import android.content.Intent;
import android.content.pm.ApplicationInfo;

/** 候选内存运行时的公开框架入口，并完整委托原应用组件工厂。 */
public final class StubComponentFactory extends AppComponentFactory {
    private static volatile FactoryState activeState;

    @Override
    public ClassLoader instantiateClassLoader(ClassLoader defaultLoader, ApplicationInfo info) {
        FactoryState state = new FactoryState(new DeferredPayloadClassLoader(defaultLoader));
        activeState = state;
        return state.proxy;
    }

    static ClassLoader initialize(Context context, String originalFactoryClass) throws Exception {
        FactoryState state = requireState();
        synchronized (state) {
            if (state.delegate != null) return state.proxy;
            ClassLoader payload = MemoryPayloadLoader.create(context, state.proxy.getParent());
            AppComponentFactory delegate = createDelegate(payload, originalFactoryClass);
            state.proxy.initialize(payload);
            state.delegate = delegate;
            return state.proxy;
        }
    }

    static Application instantiateOriginalApplication(ClassLoader loader, String className)
            throws Exception {
        return requireDelegate(requireState()).instantiateApplication(loader, className);
    }

    @Override
    public Application instantiateApplication(ClassLoader loader, String className)
            throws InstantiationException, IllegalAccessException, ClassNotFoundException {
        if (StubApp.class.getName().equals(className)) {
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

    private static AppComponentFactory createDelegate(ClassLoader loader, String className)
            throws Exception {
        if (className == null || className.trim().isEmpty()) return new AppComponentFactory();
        if (StubComponentFactory.class.getName().equals(className)) {
            throw new IllegalStateException("M05");
        }
        Class<?> type = loader.loadClass(className);
        if (!AppComponentFactory.class.isAssignableFrom(type)) {
            throw new IllegalStateException("M06");
        }
        return (AppComponentFactory) type.getDeclaredConstructor().newInstance();
    }

    private static FactoryState requireState() {
        FactoryState state = activeState;
        if (state == null) throw new IllegalStateException("M07");
        return state;
    }

    private static AppComponentFactory requireDelegate(FactoryState state) {
        AppComponentFactory delegate = state.delegate;
        if (delegate == null) throw new IllegalStateException("M08");
        return delegate;
    }

    private static final class FactoryState {
        final DeferredPayloadClassLoader proxy;
        volatile AppComponentFactory delegate;

        FactoryState(DeferredPayloadClassLoader proxy) {
            this.proxy = proxy;
        }
    }
}

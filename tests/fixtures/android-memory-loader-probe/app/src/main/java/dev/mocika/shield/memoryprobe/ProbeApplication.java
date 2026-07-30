package dev.mocika.shield.memoryprobe;

import android.app.Application;
import android.content.Context;
import android.content.pm.ApplicationInfo;
import android.os.Bundle;
import android.util.Log;

import java.lang.reflect.Field;
import java.lang.reflect.Method;
import java.util.concurrent.CountDownLatch;
import java.util.concurrent.atomic.AtomicReference;

/** 仅用于验证 API 29+ 框架 ClassLoader 替换边界，不进入正式壳。 */
public final class ProbeApplication extends Application {
    private static final String TAG = "MOCIKA_MEMORY_PROBE";
    private Application realApplication;

    @Override
    protected void attachBaseContext(Context base) {
        super.attachBaseContext(base);
        try {
            ClassLoader original = base.getClassLoader();
            ClassLoader businessLoader;
            if (original instanceof DeferredPayloadClassLoader) {
                String originalFactory = readMetadata(base, "ORIGINAL_COMPONENT_FACTORY");
                businessLoader = ProbeAppComponentFactory.initializePayload(
                        base, originalFactory);
                ClassLoader repeated = ProbeAppComponentFactory.initializePayload(
                        base, originalFactory);
                if (repeated != businessLoader) {
                    throw new IllegalStateException("MEMORY_PROBE_REINITIALIZE_CHANGED_LOADER");
                }
                verifyConcurrentBusinessLoad(
                        businessLoader, readMetadata(base, "PROBE_CONCURRENT_CLASSES"));
                Log.i(TAG, "LOADER_READY:FACTORY");
            } else {
                exemptHiddenApi();
                businessLoader = MemoryPayloadLoader.create(
                        base, original.getParent());
                replaceFrameworkClassLoader(base, businessLoader);
                Log.i(TAG, "LOADER_READY:REFLECTION");
            }
            Thread.currentThread().setContextClassLoader(businessLoader);
            realApplication = createRealApplication(base, businessLoader);
        } catch (Exception error) {
            throw new RuntimeException("MEMORY_PROBE_INIT", error);
        }
    }

    @Override
    public void onCreate() {
        super.onCreate();
        if (realApplication == null) {
            throw new IllegalStateException("MEMORY_PROBE_REAL_APP_MISSING");
        }
        try {
            replaceApplicationReferences(realApplication);
            ARouterProbeVerifier.prepareIfPresent(this);
            realApplication.onCreate();
            ARouterProbeVerifier.scheduleNavigation(
                    this, realApplication, readMetadata(this, "PROBE_AROUTER_ROUTE"));
            MemoryProbeMetrics.schedulePostStartupSnapshot();
        } catch (Exception error) {
            throw new RuntimeException("MEMORY_PROBE_APP_REPLACE", error);
        }
    }

    @Override
    public Context getApplicationContext() {
        return realApplication != null ? realApplication : super.getApplicationContext();
    }

    private static void replaceFrameworkClassLoader(Context base, ClassLoader replacement)
            throws Exception {
        Object loadedApk = findField(base.getClass(), "mPackageInfo").get(base);
        if (loadedApk == null) {
            throw new IllegalStateException("MEMORY_PROBE_LOADED_APK_MISSING");
        }
        findField(loadedApk.getClass(), "mClassLoader").set(loadedApk, replacement);
        if (base.getClassLoader() != replacement) {
            throw new IllegalStateException("MEMORY_PROBE_CONTEXT_LOADER_STALE");
        }
    }

    private Application createRealApplication(Context base, ClassLoader loader) throws Exception {
        String className = readMetadata(base, "REAL_APPLICATION");
        if (className == null || className.isEmpty()) {
            throw new IllegalStateException("MEMORY_PROBE_REAL_APP_NAME_MISSING");
        }
        Application application = ProbeAppComponentFactory.instantiateOriginalApplication(
                loader, className);
        Method attach = Application.class.getDeclaredMethod("attach", Context.class);
        attach.setAccessible(true);
        attach.invoke(application, base);
        return application;
    }

    private static String readMetadata(Context context, String key) throws Exception {
        ApplicationInfo info = context.getPackageManager().getApplicationInfo(
                context.getPackageName(), ApplicationInfo.FLAG_HAS_CODE | 128);
        Bundle metadata = info.metaData;
        return metadata == null ? null : metadata.getString(key);
    }

    private void replaceApplicationReferences(Application replacement) throws Exception {
        Class<?> activityThreadClass = Class.forName("android.app.ActivityThread");
        Method current = activityThreadClass.getDeclaredMethod("currentActivityThread");
        current.setAccessible(true);
        Object activityThread = current.invoke(null);
        findField(activityThreadClass, "mInitialApplication").set(activityThread, replacement);

        Object loadedApk = findField(getBaseContext().getClass(), "mPackageInfo")
                .get(getBaseContext());
        findField(loadedApk.getClass(), "mApplication").set(loadedApk, replacement);
    }

    private static Field findField(Class<?> type, String name) throws NoSuchFieldException {
        for (Class<?> current = type; current != null; current = current.getSuperclass()) {
            try {
                Field field = current.getDeclaredField(name);
                field.setAccessible(true);
                return field;
            } catch (NoSuchFieldException ignored) {
            }
        }
        throw new NoSuchFieldException(name);
    }

    private static void exemptHiddenApi() {
        try {
            Class<?> runtimeClass = Class.forName("dalvik.system.VMRuntime");
            Method getRuntime = runtimeClass.getDeclaredMethod("getRuntime");
            getRuntime.setAccessible(true);
            Object runtime = getRuntime.invoke(null);
            Method exemptions = runtimeClass.getDeclaredMethod(
                    "setHiddenApiExemptions", String[].class);
            exemptions.setAccessible(true);
            exemptions.invoke(runtime, (Object) new String[] {"L"});
        } catch (Exception ignored) {
        }
    }

    private static void verifyConcurrentBusinessLoad(ClassLoader loader, String classNames)
            throws Exception {
        if (classNames == null || classNames.trim().isEmpty()) {
            throw new IllegalStateException("MEMORY_PROBE_CONCURRENT_CLASSES_MISSING");
        }
        String[] names = classNames.split(",", -1);
        if (names.length != 2 || names[0].trim().isEmpty() || names[1].trim().isEmpty()) {
            throw new IllegalStateException("MEMORY_PROBE_CONCURRENT_CLASSES_INVALID");
        }
        CountDownLatch start = new CountDownLatch(1);
        AtomicReference<Throwable> failure = new AtomicReference<>();
        Thread activityLoad = classLoadThread(
                loader, names[0].trim(), start, failure);
        Thread serviceLoad = classLoadThread(
                loader, names[1].trim(), start, failure);
        activityLoad.start();
        serviceLoad.start();
        start.countDown();
        activityLoad.join(5_000);
        serviceLoad.join(5_000);
        if (activityLoad.isAlive() || serviceLoad.isAlive()) {
            throw new IllegalStateException("MEMORY_PROBE_CONCURRENT_LOAD_TIMEOUT");
        }
        if (failure.get() != null) {
            throw new RuntimeException("MEMORY_PROBE_CONCURRENT_LOAD_FAILED", failure.get());
        }
        Log.i(TAG, "CONCURRENT_LOAD_OK");
    }

    private static Thread classLoadThread(
            ClassLoader loader,
            String className,
            CountDownLatch start,
            AtomicReference<Throwable> failure) {
        return new Thread(() -> {
            try {
                start.await();
                loader.loadClass(className);
            } catch (Throwable error) {
                failure.compareAndSet(null, error);
            }
        }, "memory-probe-load");
    }
}

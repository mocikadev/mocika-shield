package dev.mocika.shield.memoryprobe;

import android.app.Application;
import android.content.Context;
import android.content.pm.ApplicationInfo;
import android.os.Bundle;
import android.util.Log;

import java.lang.reflect.Field;
import java.lang.reflect.Method;

import dalvik.system.InMemoryDexClassLoader;

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
            if (original instanceof InMemoryDexClassLoader) {
                businessLoader = original;
                Log.i(TAG, "LOADER_READY:FACTORY");
            } else {
                exemptHiddenApi();
                businessLoader = MemoryPayloadLoader.create(
                        base.getApplicationInfo(), original.getParent());
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
            realApplication.onCreate();
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
        ApplicationInfo info = getPackageManager().getApplicationInfo(
                getPackageName(), ApplicationInfo.FLAG_HAS_CODE | 128);
        Bundle metadata = info.metaData;
        String className = metadata == null ? null : metadata.getString("REAL_APPLICATION");
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
}

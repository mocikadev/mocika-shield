package dev.mocika.shield.memoryprobe;

import android.app.Application;
import android.content.Context;
import android.content.pm.ApplicationInfo;
import android.os.Bundle;
import android.os.Build;
import android.util.Log;

import java.io.ByteArrayOutputStream;
import java.io.InputStream;
import java.lang.reflect.Field;
import java.lang.reflect.Method;
import java.nio.ByteBuffer;

import dalvik.system.InMemoryDexClassLoader;

/** 仅用于验证 API 29+ 框架 ClassLoader 替换边界，不进入正式壳。 */
public final class ProbeApplication extends Application {
    private static final String TAG = "MOCIKA_MEMORY_PROBE";
    private Application realApplication;

    @Override
    protected void attachBaseContext(Context base) {
        super.attachBaseContext(base);
        try {
            exemptHiddenApi();
            ClassLoader original = base.getClassLoader();
            ByteBuffer[] payload = new ByteBuffer[] {
                    readDirectBuffer(base, "payload-main.dex"),
                    readDirectBuffer(base, "payload-secondary.dex")
            };
            String nativePath = buildNativeSearchPath(base.getApplicationInfo());
            ClassLoader replacement = new InMemoryDexClassLoader(
                    payload, nativePath, original.getParent());
            replaceFrameworkClassLoader(base, replacement);
            Thread.currentThread().setContextClassLoader(replacement);
            realApplication = createRealApplication(base, replacement);
            Log.i(TAG, "LOADER_REPLACED");
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

    private static ByteBuffer readDirectBuffer(Context context, String name) throws Exception {
        try (InputStream input = context.getAssets().open(name);
             ByteArrayOutputStream output = new ByteArrayOutputStream()) {
            byte[] chunk = new byte[16 * 1024];
            int count;
            while ((count = input.read(chunk)) >= 0) {
                output.write(chunk, 0, count);
            }
            byte[] bytes = output.toByteArray();
            ByteBuffer buffer = ByteBuffer.allocateDirect(bytes.length);
            buffer.put(bytes);
            buffer.flip();
            return buffer;
        }
    }

    private static String buildNativeSearchPath(ApplicationInfo info) {
        StringBuilder paths = new StringBuilder();
        for (String abi : Build.SUPPORTED_ABIS) {
            if (paths.length() > 0) {
                paths.append(java.io.File.pathSeparatorChar);
            }
            paths.append(info.sourceDir).append("!/lib/").append(abi);
        }
        if (info.nativeLibraryDir != null && !info.nativeLibraryDir.isEmpty()) {
            paths.append(java.io.File.pathSeparatorChar).append(info.nativeLibraryDir);
        }
        return paths.toString();
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
        Application application = (Application) loader.loadClass(className)
                .getDeclaredConstructor().newInstance();
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

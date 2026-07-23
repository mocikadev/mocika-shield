package dev.mocika.shield.loader;

import android.app.Application;
import android.content.Context;
import android.content.pm.ApplicationInfo;
import android.content.pm.PackageManager;
import android.util.Log;

import java.io.File;
import java.lang.ref.WeakReference;
import java.lang.reflect.Array;
import java.lang.reflect.Field;
import java.lang.reflect.Method;
import java.util.ArrayList;
import java.util.List;
import java.util.Map;

public class StubApp extends Application {

    private static final String TAG = "ax";

    private Application realApp;

    @Override
    protected void attachBaseContext(Context base) {
        super.attachBaseContext(base);
        try {
            exemptHiddenApi();
            List<File> dexFiles = Ld.extractDexFiles(base);
            injectDexElements(base, dexFiles);
            realApp = makeRealApp(base.getClassLoader(), base);
        } catch (Exception e) {
            throw new RuntimeException("init", e);
        }
    }

    @Override
    public void onCreate() {
        super.onCreate();
        if (realApp == null) return;
        replaceAppReferences(realApp);
        ARouterCompat.prepareARouterRouteMap(this);
        realApp.onCreate();
    }

    @Override
    public Context getApplicationContext() {
        return realApp != null ? realApp : super.getApplicationContext();
    }

    /**
     * 将落地的 DEX 文件注入原始 PathClassLoader 的 dexElements。
     *
     * 优先通过 JNI 路径（Ld.p）完成注入：JNI 访问字段/方法不经过
     * hidden API 检查，面向 Android 15+ 更健壮。JNI 失败时降级到 Java 反射方案。
     *
     * addDexPath 内部将 DEX 直接注册到 PathClassLoader（definingContext），
     * 整个过程不创建任何中间 ClassLoader，从根源上避免：
     *   - "multiple class loaders" InternalError（同一 DEX 被多个 loader 注册）
     *   - InstantiationError（lambda access check 因 defining loader 不一致而失败）
     */
    private void injectDexElements(Context base, List<File> dexFiles) throws Exception {
        ClassLoader pathCl = base.getClassLoader();

        String[] dexPaths = new String[dexFiles.size()];
        for (int i = 0; i < dexFiles.size(); i++) {
            dexPaths[i] = dexFiles.get(i).getAbsolutePath();
        }
        String optDirPath = (android.os.Build.VERSION.SDK_INT < 26)
                ? base.getDir("dex_opt", Context.MODE_PRIVATE).getAbsolutePath() : "";

        boolean nativeOk = Ld.p(pathCl, dexPaths, optDirPath);
        if (!nativeOk) {
            injectDexElementsJavaFallback(base, pathCl, dexFiles);
        }
    }

    private void injectDexElementsJavaFallback(Context base, ClassLoader pathCl, List<File> dexFiles)
            throws Exception {
        Field pathListField = findField(pathCl.getClass(), "pathList");
        pathListField.setAccessible(true);
        Object pathList = pathListField.get(pathCl);

        Field dexElementsField = findField(pathList.getClass(), "dexElements");
        dexElementsField.setAccessible(true);
        int oldLen = ((Object[]) dexElementsField.get(pathList)).length;

        Method addDexPath = findMethod(pathList.getClass(), "addDexPath",
                String.class, File.class);
        File optDir = (android.os.Build.VERSION.SDK_INT < 26)
                ? base.getDir("dex_opt", Context.MODE_PRIVATE) : null;

        for (File dexFile : dexFiles) {
            addDexPath.invoke(pathList, dexFile.getAbsolutePath(), optDir);
        }

        Object[] elements = (Object[]) dexElementsField.get(pathList);
        int injected = elements.length - oldLen;
        if (injected > 0) {
            Object[] reordered = (Object[]) Array.newInstance(
                    elements.getClass().getComponentType(), elements.length);
            System.arraycopy(elements, oldLen, reordered, 0, injected);
            System.arraycopy(elements, 0, reordered, injected, oldLen);
            dexElementsField.set(pathList, reordered);
        }
    }

    private static Method findMethod(Class<?> clazz, String name, Class<?>... params)
            throws NoSuchMethodException {
        for (Class<?> c = clazz; c != null; c = c.getSuperclass()) {
            try {
                Method m = c.getDeclaredMethod(name, params);
                m.setAccessible(true);
                return m;
            } catch (NoSuchMethodException ignored) {}
        }
        throw new NoSuchMethodException(name);
    }

    private static Field findField(Class<?> clazz, String name) throws NoSuchFieldException {
        for (Class<?> c = clazz; c != null; c = c.getSuperclass()) {
            try {
                return c.getDeclaredField(name);
            } catch (NoSuchFieldException ignored) {}
        }
        throw new NoSuchFieldException(name);
    }

    private Application makeRealApp(ClassLoader cl, Context base) throws Exception {
        String name = getRealAppName();
        Application app = (Application) cl.loadClass(name).newInstance();
        Method attach = Application.class.getDeclaredMethod("attach", Context.class);
        attach.setAccessible(true);
        attach.invoke(app, base);
        return app;
    }

    private String getRealAppName() {
        try {
            ApplicationInfo ai = getPackageManager().getApplicationInfo(
                    getPackageName(), PackageManager.GET_META_DATA);
            if (ai.metaData != null) {
                String v = ai.metaData.getString("ORIGINAL_APPLICATION");
                if (v != null && !v.isEmpty()) return v;
            }
        } catch (Exception ignored) {}
        return "android.app.Application";
    }

    private void replaceAppReferences(Application newApp) {
        try {
            Class<?> atCls = Class.forName("android.app.ActivityThread");
            Method current = atCls.getDeclaredMethod("currentActivityThread");
            current.setAccessible(true);
            Object at = current.invoke(null);

            // mInitialApplication 替换失败为致命错误：影响 Application.getApplicationContext() 返回值
            try {
                Field f = atCls.getDeclaredField("mInitialApplication");
                f.setAccessible(true);
                f.set(at, newApp);
            } catch (Exception e) {
                throw new RuntimeException("e1", e);
            }

            // mAllApplications 替换失败可容忍（仅影响 ActivityThread.mAllApplications 列表）
            try {
                Field f = atCls.getDeclaredField("mAllApplications");
                f.setAccessible(true);
                @SuppressWarnings("unchecked")
                ArrayList<Application> list = (ArrayList<Application>) f.get(at);
                if (list != null) {
                    for (int i = 0; i < list.size(); i++) {
                        if (list.get(i) == this) list.set(i, newApp);
                    }
                }
            } catch (NoSuchFieldException ignored) {}

            // mPackages→mApplication 替换失败为致命错误：影响后续 Context.getApplicationContext()
            try {
                Field mPackages = atCls.getDeclaredField("mPackages");
                mPackages.setAccessible(true);
                Map<?, ?> pkgs = (Map<?, ?>) mPackages.get(at);
                WeakReference<?> ref = (WeakReference<?>) pkgs.get(getPackageName());
                if (ref != null && ref.get() != null) {
                    setField(ref.get().getClass(), ref.get(), "mApplication", newApp);
                }
            } catch (Exception e) {
                throw new RuntimeException("e2", e);
            }
        } catch (RuntimeException e) {
            throw e;
        } catch (Exception e) {
            throw new RuntimeException("e3", e);
        }
    }

    private static void setField(Class<?> cls, Object obj, String name, Object val) {
        try {
            Field f = cls.getDeclaredField(name);
            f.setAccessible(true);
            f.set(obj, val);
        } catch (NoSuchFieldException ignored) {
        } catch (Exception e) {
            Log.w(TAG, "setField " + name + " failed: " + e);
        }
    }

    private static void exemptHiddenApi() {
        if (android.os.Build.VERSION.SDK_INT < 28) return;
        try {
            Class<?> vr = Class.forName("dalvik.system.VMRuntime");
            Method gr = vr.getDeclaredMethod("getRuntime");
            gr.setAccessible(true);
            Object runtime = gr.invoke(null);
            Method se = vr.getDeclaredMethod("setHiddenApiExemptions", String[].class);
            se.setAccessible(true);
            se.invoke(runtime, (Object) new String[]{"L"});
        } catch (Exception ignored) {}
    }
}

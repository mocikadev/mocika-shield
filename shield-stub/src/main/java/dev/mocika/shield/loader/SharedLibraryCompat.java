package dev.mocika.shield.loader;

import android.content.Context;
import android.content.pm.ApplicationInfo;
import android.os.Build;

import java.io.File;
import java.io.IOException;
import java.util.Enumeration;
import java.util.List;

import dalvik.system.DexFile;

/** 恢复 Android 9 动态 DEX 注入前的 Apache HTTP 共享库解析优先级。 */
final class SharedLibraryCompat {

    private static final String LEGACY_HTTP_LIBRARY = "org.apache.http.legacy";
    private static final String[] LEGACY_HTTP_PACKAGES = {
            "org.apache.http.",
            "android.net.http."
    };

    private SharedLibraryCompat() {}

    static void prepare(Context context, List<File> dexFiles) throws IOException {
        ApplicationInfo appInfo = context.getApplicationInfo();
        if (!shouldPrepare(Build.VERSION.SDK_INT, appInfo.sharedLibraryFiles)) return;

        ClassLoader appClassLoader = context.getClassLoader();
        for (File dexPath : dexFiles) {
            DexFile dexFile = new DexFile(dexPath);
            try {
                Enumeration<String> entries = dexFile.entries();
                while (entries.hasMoreElements()) {
                    String className = entries.nextElement();
                    if (!isLegacyHttpClass(className)) continue;
                    try {
                        appClassLoader.loadClass(className);
                    } catch (ClassNotFoundException | LinkageError ignored) {}
                }
            } finally {
                dexFile.close();
            }
        }
    }

    static boolean shouldPrepare(int sdkInt, String[] sharedLibraryFiles) {
        if (sdkInt != 28 || sharedLibraryFiles == null) return false;
        for (String path : sharedLibraryFiles) {
            if (path != null && path.contains(LEGACY_HTTP_LIBRARY)) return true;
        }
        return false;
    }

    static boolean isLegacyHttpClass(String className) {
        if (className == null) return false;
        for (String prefix : LEGACY_HTTP_PACKAGES) {
            if (className.startsWith(prefix)) return true;
        }
        return false;
    }
}

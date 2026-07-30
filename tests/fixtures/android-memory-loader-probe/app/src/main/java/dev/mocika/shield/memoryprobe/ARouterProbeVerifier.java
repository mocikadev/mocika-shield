package dev.mocika.shield.memoryprobe;

import android.app.Application;
import android.content.Context;
import android.os.Handler;
import android.os.Looper;
import android.util.Log;

/** 仅负责真实 ARouter 样本的准备与自动路由断言。 */
final class ARouterProbeVerifier {
    private static final String TAG = "MOCIKA_MEMORY_PROBE";

    private ARouterProbeVerifier() {}

    static void prepareIfPresent(Context context) throws Exception {
        try {
            Class<?> compatibility = Class.forName(
                    "dev.mocika.shield.loader.ARouterCompat",
                    false,
                    ARouterProbeVerifier.class.getClassLoader());
            compatibility.getMethod("prepareARouterRouteMap", Context.class)
                    .invoke(null, context);
            Log.i(TAG, "AROUTER_PREPARE_OK");
        } catch (ClassNotFoundException ignored) {
        }
    }

    static void scheduleNavigation(
            Context context, Application realApplication, String route) {
        if (route == null || route.trim().isEmpty()) {
            return;
        }
        String normalizedRoute = route.trim();
        new Handler(Looper.getMainLooper()).postDelayed(() -> {
            try {
                ClassLoader loader = realApplication.getClass().getClassLoader();
                Class<?> arouterClass = loader.loadClass(
                        "com.alibaba.android.arouter.launcher.ARouter");
                Object arouter = arouterClass.getMethod("getInstance").invoke(null);
                Object postcard = arouterClass.getMethod("build", String.class)
                        .invoke(arouter, normalizedRoute);
                postcard.getClass().getMethod("navigation", Context.class)
                        .invoke(postcard, context);
                Log.i(TAG, "AROUTER_NAVIGATION_INVOKED:" + normalizedRoute);
            } catch (Exception error) {
                Log.e(TAG, "AROUTER_NAVIGATION_FAILED", error);
            }
        }, 500L);
    }
}

package dev.mocika.shield.loader;

import android.content.Context;
import android.content.SharedPreferences;
import android.content.pm.ApplicationInfo;
import android.content.pm.PackageInfo;
import android.content.pm.PackageManager;
import android.util.Log;

import java.io.BufferedReader;
import java.io.InputStream;
import java.io.InputStreamReader;
import java.lang.reflect.Method;
import java.util.ArrayList;
import java.util.LinkedHashSet;
import java.util.List;
import java.util.Set;

public class ARouterCompat {

    private static final String TAG = "rx";
    private static final String AROUTER_ROUTES_PACKAGE = "com.alibaba.android.arouter.routes.";
    private static final String ROUTE_LIST_ASSET = "arouter_routes.txt";
    private static final String CACHE_NAME = "SP_AROUTER_CACHE";
    private static final String CACHE_ROUTE_MAP = "ROUTER_MAP";
    private static final String CACHE_VERSION_NAME = "LAST_VERSION_NAME";
    private static final String CACHE_VERSION_CODE = "LAST_VERSION_CODE";

    /**
     * 从 assets/arouter_routes.txt 读取路由表类名并写入 ARouter 自有缓存。
     * 路由表由 shield-cli 加固时静态扫描 DEX 生成，不依赖 DexFile API，兼容 API 21 及以上。
     * 必须在宿主 Application.onCreate() 之前调用，让 ARouter.init() 按正常流程仅加载当前路由表一次。
     */
    public static void prepareARouterRouteMap(Context context) {
        try {
            Class<?> logisticsCenterClass = Class.forName(
                    "com.alibaba.android.arouter.core.LogisticsCenter",
                    false,
                    context.getClassLoader());

            Set<String> routeClassNames = readRouteList(context);
            if (routeClassNames.isEmpty()) {
                return;
            }

            PackageInfo packageInfo = context.getPackageManager().getPackageInfo(
                    context.getPackageName(), 0);
            SharedPreferences.Editor editor = context
                    .getSharedPreferences(CACHE_NAME, Context.MODE_PRIVATE)
                    .edit()
                    .putStringSet(CACHE_ROUTE_MAP, new LinkedHashSet<>(routeClassNames))
                    .putString(CACHE_VERSION_NAME, packageInfo.versionName)
                    .putInt(CACHE_VERSION_CODE, packageInfo.versionCode);

            // ARouter.init() 紧接着读取缓存，必须同步落盘，不能使用异步 apply()。
            if (!editor.commit()) {
                Log.w(TAG, "A01");
            }

            // 可调试包会忽略版本缓存并强制扫描 APK。加密 DEX 不在 sourceDir 中，
            // 因此保留提前注册路径，避免首次安装或清除数据后路由表为空。
            if (shouldPreRegister(context.getApplicationInfo().flags)) {
                registerRoutes(logisticsCenterClass, routeClassNames);
            }
        } catch (ClassNotFoundException ignored) {
        } catch (PackageManager.NameNotFoundException e) {
            Log.w(TAG, "A02", e);
        } catch (Exception e) {
            Log.e(TAG, "A03", e);
        }
    }

    private static void registerRoutes(Class<?> logisticsCenterClass, Set<String> routeClassNames)
            throws Exception {
        Method registerMethod = logisticsCenterClass.getDeclaredMethod("register", String.class);
        registerMethod.setAccessible(true);
        for (String className : routeClassNames) {
            try {
                registerMethod.invoke(null, className);
            } catch (Exception e) {
                Log.w(TAG, "A04", e);
            }
        }
    }

    static boolean shouldPreRegister(int applicationFlags) {
        return (applicationFlags & ApplicationInfo.FLAG_DEBUGGABLE) != 0;
    }

    private static Set<String> readRouteList(Context context) {
        List<String> lines = new ArrayList<>();
        try (InputStream is = context.getAssets().open(ROUTE_LIST_ASSET);
             BufferedReader reader = new BufferedReader(new InputStreamReader(is, "UTF-8"))) {
            String line;
            while ((line = reader.readLine()) != null) {
                lines.add(line);
            }
        } catch (java.io.FileNotFoundException e) {
            // 文件不存在：宿主未使用 ARouter 或已通过 arouter-register plugin 编译期注入，属正常情况
        } catch (Exception e) {
            Log.w(TAG, "A05", e);
        }
        return filterRouteClassNames(lines);
    }

    static Set<String> filterRouteClassNames(Iterable<String> lines) {
        Set<String> names = new LinkedHashSet<>();
        for (String line : lines) {
            if (line == null) {
                continue;
            }
            String className = line.trim();
            if (className.startsWith(AROUTER_ROUTES_PACKAGE)) {
                names.add(className);
            }
        }
        return names;
    }
}

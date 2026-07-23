package dev.mocika.shield.loader;

import android.content.Context;
import android.util.Log;

import java.io.BufferedReader;
import java.io.InputStream;
import java.io.InputStreamReader;
import java.lang.reflect.Method;
import java.util.ArrayList;
import java.util.List;

public class ARouterCompat {

    private static final String TAG = "rx";
    private static final String AROUTER_ROUTES_PACKAGE = "com.alibaba.android.arouter.routes.";
    private static final String ROUTE_LIST_ASSET = "arouter_routes.txt";

    /**
     * 从 assets/arouter_routes.txt 读取路由表类名并注册到 ARouter Warehouse。
     * 路由表由 shield-cli 加固时静态扫描 DEX 生成，不依赖 DexFile API，兼容 API 24-34。
     * 必须在宿主 Application.onCreate() 之前调用，确保 ARouter.init() 能初始化内置服务。
     */
    public static void prepareARouterRouteMap(Context context) {
        try {
            Class<?> logisticsCenterClass = Class.forName(
                    "com.alibaba.android.arouter.core.LogisticsCenter");

            java.lang.reflect.Field registerByPlugin = null;
            try {
                registerByPlugin = logisticsCenterClass.getDeclaredField("registerByPlugin");
                registerByPlugin.setAccessible(true);
                if ((boolean) registerByPlugin.get(null)) {
                    return;
                }
            } catch (NoSuchFieldException ignored) {
                // 旧版 ARouter 无此字段，继续走补注册逻辑
            }

            List<String> routeClassNames = readRouteList(context);
            if (routeClassNames.isEmpty()) {
                return;
            }

            Method registerMethod = logisticsCenterClass.getDeclaredMethod("register", String.class);
            registerMethod.setAccessible(true);

            int registeredCount = 0;
            for (String className : routeClassNames) {
                try {
                    registerMethod.invoke(null, className);
                    registeredCount++;
                } catch (Exception e) {
                    Log.w(TAG, "  注册路由类失败: " + className, e);
                }
            }

            // 告知 LogisticsCenter 路由表已预注册，避免首次安装时再次扫描壳 DEX。
            if (registerByPlugin != null && registeredCount > 0) {
                registerByPlugin.setBoolean(null, true);
            }

        } catch (ClassNotFoundException ignored) {
        } catch (Exception e) {
            Log.e(TAG, "ARouter 路由表补注册失败", e);
        }
    }

    private static List<String> readRouteList(Context context) {
        List<String> names = new ArrayList<>();
        try (InputStream is = context.getAssets().open(ROUTE_LIST_ASSET);
             BufferedReader reader = new BufferedReader(new InputStreamReader(is, "UTF-8"))) {
            String line;
            while ((line = reader.readLine()) != null) {
                line = line.trim();
                if (!line.isEmpty() && line.startsWith(AROUTER_ROUTES_PACKAGE)) {
                    names.add(line);
                }
            }
        } catch (java.io.FileNotFoundException e) {
            // 文件不存在：宿主未使用 ARouter 或已通过 arouter-register plugin 编译期注入，属正常情况
        } catch (Exception e) {
            Log.w(TAG, "读取 " + ROUTE_LIST_ASSET + " 失败", e);
        }
        return names;
    }
}

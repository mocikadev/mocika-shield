package dev.mocika.shield.loader;

import android.content.Context;
import android.content.pm.ApplicationInfo;
import android.content.pm.PackageManager;

/** 负责在每次进程启动时执行环境安全检查，不参与 DEX 缓存与类加载。 */
final class RuntimeSecurity {

    private static final int SIGNAL_ANTI_DEBUG = 1;
    private static final int SIGNAL_ROOT = 1 << 1;
    private static final String POLICY_KEY = "dev.mocika.shield.ENV_POLICY";

    private RuntimeSecurity() {
    }

    static void checkEnvironment(Context context) {
        int signals = Ld.r();
        enforceSafe(shouldReject(signals, isStrictPolicy(context)));
    }

    static boolean shouldReject(int signals, boolean strict) {
        return (signals & SIGNAL_ANTI_DEBUG) != 0
                || (strict && (signals & SIGNAL_ROOT) != 0);
    }

    static boolean isStrictPolicy(Context context) {
        try {
            ApplicationInfo info = context.getPackageManager().getApplicationInfo(
                    context.getPackageName(), PackageManager.GET_META_DATA);
            return info.metaData != null && "strict".equals(info.metaData.getString(POLICY_KEY));
        } catch (Exception ignored) {
            return false;
        }
    }

    static void enforceSafe(boolean unsafe) {
        if (unsafe) {
            throw new SecurityException("S01");
        }
    }
}

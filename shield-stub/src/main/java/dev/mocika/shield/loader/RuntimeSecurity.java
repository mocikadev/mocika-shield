package dev.mocika.shield.loader;

import android.content.Context;

/** 负责在每次进程启动时执行环境安全检查，不参与 DEX 缓存与类加载。 */
final class RuntimeSecurity {

    private static final int SIGNAL_ANTI_DEBUG = 1;
    private static final int SIGNAL_ROOT = 1 << 1;

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
        return parsePolicy(RuntimeManifest.read(context).get(ManifestKeys.POLICY));
    }

    static boolean parsePolicy(Object value) {
        if ("strict".equals(value)) return true;
        if ("compatible".equals(value)) return false;
        throw new SecurityException("S02");
    }

    static void enforceSafe(boolean unsafe) {
        if (unsafe) {
            throw new SecurityException("S01");
        }
    }
}

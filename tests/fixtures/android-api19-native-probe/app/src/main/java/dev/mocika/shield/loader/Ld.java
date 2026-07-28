package dev.mocika.shield.loader;

import android.content.Context;

/** 只用于验证生产 Native Stub 的 JNI_OnLoad 动态注册，不包含加固业务逻辑。 */
public final class Ld {
    private Ld() {
    }

    public static native boolean p(
            ClassLoader classLoader,
            String[] dexPaths,
            String optimizedDirectory);

    public static native byte[][] q(Context context, byte[] encryptedDex);
}

package dev.mocika.shield.smoke;

/** 声明动态注册的 Native 方法，并提供可独立占位的 Java 包装层。 */
public final class JniDexSeparationBridge {
    static {
        System.loadLibrary("dexresearch");
    }

    private JniDexSeparationBridge() {}

    private static native String nativeRoundTrip(int input);

    public static String wrapper(int input) {
        return nativeRoundTrip(input);
    }
}

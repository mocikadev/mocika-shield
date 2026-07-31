package dev.mocika.shield.smoke;

/** 由 Native 回调、可独立占位的 Java 业务方法。 */
public final class JniDexSeparationCallback {
    private JniDexSeparationCallback() {}

    public static String value(int input) {
        return "java-" + (input + 4);
    }
}

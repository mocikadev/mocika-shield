package dev.mocika.shield.smoke;

/** 观察 Java、动态注册 Native 与 Native 回调 Java 的完整链路。 */
public final class JniDexSeparationReporter {
    private JniDexSeparationReporter() {}

    public static String snapshot() {
        try {
            return JniDexSeparationBridge.wrapper(10);
        } catch (RuntimeException | LinkageError error) {
            return "error:" + error.getClass().getSimpleName();
        }
    }
}

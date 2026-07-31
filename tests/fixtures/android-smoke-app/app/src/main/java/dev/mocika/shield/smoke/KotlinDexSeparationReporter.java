package dev.mocika.shield.smoke;

/** 保持在结构载体中的 Kotlin 观察入口，不参与方法指令抽取。 */
public final class KotlinDexSeparationReporter {
    private KotlinDexSeparationReporter() {}

    public static String snapshot() {
        return "default=" + KotlinDexSeparationCases.defaultValue("盾", 4)
                + ",lambda=" + KotlinDexSeparationCases.lambdaValue(6)
                + ",synthetic=" + KotlinDexSeparationCases.syntheticValue(5)
                + ",suspend=" + suspendValue();
    }

    private static String suspendValue() {
        try {
            return String.valueOf(KotlinDexSeparationCases.suspendValueBlocking(8));
        } catch (RuntimeException error) {
            return "error:" + error.getClass().getSimpleName();
        }
    }
}

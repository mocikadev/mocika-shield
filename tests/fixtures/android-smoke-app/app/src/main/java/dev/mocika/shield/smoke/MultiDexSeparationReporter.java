package dev.mocika.shield.smoke;

import java.lang.reflect.Method;

/** 位于主 DEX 的跨 DEX 观察入口，不参与方法指令抽取。 */
public final class MultiDexSeparationReporter {
    private static final String SECONDARY_CASES =
            "dev.mocika.shield.smoke.SecondaryDexSeparationCases";

    private MultiDexSeparationReporter() {}

    public static String snapshot() {
        try {
            Class<?> cases = Class.forName(SECONDARY_CASES);
            Method method = cases.getDeclaredMethod("crossValue", int.class);
            return String.valueOf(method.invoke(null, 10));
        } catch (ReflectiveOperationException error) {
            throw new IllegalStateException("多 DEX 方法代码分离验证失败", error);
        }
    }
}

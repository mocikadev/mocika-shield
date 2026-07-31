package dev.mocika.shield.smoke;

import java.lang.reflect.Method;

/** 只编译到 classes2.dex，验证次 DEX 回调主 DEX。 */
public final class SecondaryDexSeparationCases {
    private static final String MAIN_CASES = "dev.mocika.shield.smoke.MultiDexMainCases";

    private SecondaryDexSeparationCases() {}

    public static String crossValue(int input) {
        try {
            Class<?> cases = Class.forName(MAIN_CASES);
            Method method = cases.getDeclaredMethod("value", int.class);
            return "secondary-" + method.invoke(null, input);
        } catch (ReflectiveOperationException error) {
            throw new IllegalStateException("次 DEX 回调主 DEX 失败", error);
        }
    }
}

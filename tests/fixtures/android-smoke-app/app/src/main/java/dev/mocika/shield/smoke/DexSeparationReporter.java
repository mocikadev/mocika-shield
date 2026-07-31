package dev.mocika.shield.smoke;

import java.lang.reflect.Method;

/** 保持在结构载体中的观察入口，不参与方法指令抽取。 */
public final class DexSeparationReporter {
    private static final String CASES = "dev.mocika.shield.smoke.DexSeparationCases";

    private DexSeparationReporter() {}

    public static String snapshot() {
        try {
            Class<?> cases = Class.forName(CASES);
            DexSeparationState.value = 0;
            call(cases, "touchState", new Class<?>[0]);
            int[] array = (int[]) call(cases, "arrayValue", new Class<?>[0]);
            return "int=" + call(cases, "integerValue", new Class<?>[0])
                    + ",long=" + call(cases, "longValue", new Class<?>[0])
                    + ",obj=" + call(cases, "objectValue", new Class<?>[0])
                    + ",array=" + (array == null ? -1 : array.length)
                    + ",switch=" + call(cases, "switchValue", new Class<?>[] {int.class}, 3)
                    + ",catch=" + call(cases, "catchValue", new Class<?>[] {int.class}, 0)
                    + ",sync=" + call(cases, "synchronizedValue", new Class<?>[0])
                    + ",bool=" + call(cases, "booleanValue", new Class<?>[0])
                    + ",double=" + call(cases, "doubleValue", new Class<?>[0])
                    + ",void=" + DexSeparationState.value;
        } catch (ReflectiveOperationException error) {
            throw new IllegalStateException("DEX 方法代码分离反射验证失败", error);
        }
    }

    private static Object call(Class<?> owner, String name, Class<?>[] parameterTypes,
                               Object... arguments) throws ReflectiveOperationException {
        Method method = owner.getDeclaredMethod(name, parameterTypes);
        return method.invoke(null, arguments);
    }
}

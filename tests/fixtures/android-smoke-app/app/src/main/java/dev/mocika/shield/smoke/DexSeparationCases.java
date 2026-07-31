package dev.mocika.shield.smoke;

/** DEX 方法代码分离的纯业务语义样本。 */
public final class DexSeparationCases {
    private DexSeparationCases() {}

    public static int integerValue() {
        return 37;
    }

    public static long longValue() {
        return 9_000_000_007L;
    }

    public static String objectValue() {
        return "mocika";
    }

    public static int[] arrayValue() {
        return new int[] {2, 4, 6};
    }

    public static boolean booleanValue() {
        return true;
    }

    public static double doubleValue() {
        return 3.25d;
    }

    public static int switchValue(int value) {
        switch (value) {
            case 1:
                return 11;
            case 2:
                return 22;
            case 3:
                return 33;
            default:
                return -1;
        }
    }

    public static int catchValue(int divisor) {
        try {
            return 100 / divisor;
        } catch (ArithmeticException ignored) {
            return -1;
        }
    }

    public static synchronized int synchronizedValue() {
        return 23;
    }

    public static void touchState() {
        DexSeparationState.value = 41;
    }
}

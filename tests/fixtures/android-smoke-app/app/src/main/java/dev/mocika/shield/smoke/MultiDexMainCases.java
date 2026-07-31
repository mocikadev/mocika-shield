package dev.mocika.shield.smoke;

/** 位于主 DEX 的跨 DEX 业务样本。 */
public final class MultiDexMainCases {
    private MultiDexMainCases() {}

    public static String value(int input) {
        return "main-" + (input + 7);
    }
}

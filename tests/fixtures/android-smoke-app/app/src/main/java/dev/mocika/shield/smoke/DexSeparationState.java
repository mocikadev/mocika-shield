package dev.mocika.shield.smoke;

/** 由未抽取的观察入口持有状态，避免 void 样本的读取方法被一并置空。 */
final class DexSeparationState {
    private DexSeparationState() {}

    static int value;
}

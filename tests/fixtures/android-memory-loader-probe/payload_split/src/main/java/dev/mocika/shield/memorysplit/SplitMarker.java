package dev.mocika.shield.memorysplit;

/** 仅用于确认安装时动态特性的代码由系统 split 路径加载。 */
public final class SplitMarker {
    private SplitMarker() {
    }

    public static String value() {
        return "SPLIT_CODE_OK";
    }
}

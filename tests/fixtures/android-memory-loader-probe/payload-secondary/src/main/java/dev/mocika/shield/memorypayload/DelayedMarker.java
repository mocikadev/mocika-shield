package dev.mocika.shield.memorypayload;

public final class DelayedMarker {
    private DelayedMarker() {}

    public static String value() {
        return "AFTER_GC";
    }
}

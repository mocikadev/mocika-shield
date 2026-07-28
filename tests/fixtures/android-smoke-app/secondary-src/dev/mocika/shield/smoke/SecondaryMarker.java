package dev.mocika.shield.smoke;

import android.util.Log;

/** 只编译到 classes2.dex，用于验证 Dalvik 多 DEX 注入。 */
public final class SecondaryMarker {
    private SecondaryMarker() {
    }

    public static void verify() {
        Log.i("MocikaSmoke", "MOCIKA_SMOKE_SECONDARY_OK");
    }
}

package dev.mocika.shield.api19probe;

import android.app.Application;
import android.os.Build;
import android.util.Log;

public final class ProbeApplication extends Application {
    public static final String LOG_TAG = "MocikaApi19Probe";
    private static boolean nativeLoaded;

    static {
        try {
            System.loadLibrary("mocikashield");
            nativeLoaded = true;
        } catch (Throwable error) {
            nativeLoaded = false;
            Log.e(LOG_TAG, "MOCIKA_API19_NATIVE_FAILED", error);
        }
    }

    public static boolean isNativeLoaded() {
        return nativeLoaded;
    }

    @Override
    public void onCreate() {
        super.onCreate();
        if (nativeLoaded) {
            Log.i(
                    LOG_TAG,
                    "MOCIKA_API19_NATIVE_OK sdk=" + Build.VERSION.SDK_INT
                            + " abi=" + Build.CPU_ABI);
        }
    }
}

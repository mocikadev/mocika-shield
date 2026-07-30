package dev.mocika.shield.memorypayload;

import android.app.Application;
import android.util.Log;

public final class PayloadApplication extends Application {
    @Override
    public void onCreate() {
        super.onCreate();
        String marker = SecondaryMarker.value();
        Log.i("MOCIKA_MEMORY_PROBE", "APPLICATION_OK:" + marker);
        verifyOptionalSplit();
    }

    private void verifyOptionalSplit() {
        try {
            Class<?> marker = getClassLoader().loadClass(
                    "dev.mocika.shield.memorysplit.SplitMarker");
            Object value = marker.getMethod("value").invoke(null);
            Log.i("MOCIKA_MEMORY_PROBE", "SPLIT_LOADER_OK:" + value);
        } catch (ClassNotFoundException ignored) {
            Log.i("MOCIKA_MEMORY_PROBE", "SPLIT_NOT_INSTALLED");
        } catch (Exception error) {
            throw new IllegalStateException("MEMORY_PROBE_SPLIT_FAILED", error);
        }
    }
}

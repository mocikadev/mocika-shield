package dev.mocika.shield.memorypayload;

import android.app.Application;
import android.util.Log;

public final class PayloadApplication extends Application {
    @Override
    public void onCreate() {
        super.onCreate();
        String marker = SecondaryMarker.value();
        Log.i("MOCIKA_MEMORY_PROBE", "APPLICATION_OK:" + marker);
    }
}

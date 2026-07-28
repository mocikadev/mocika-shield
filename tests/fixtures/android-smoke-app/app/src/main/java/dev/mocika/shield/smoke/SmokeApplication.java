package dev.mocika.shield.smoke;

import android.app.Application;
import android.util.Log;

public final class SmokeApplication extends Application {
    @Override
    public void onCreate() {
        super.onCreate();
        Log.i("MocikaSmoke", "MOCIKA_SMOKE_APPLICATION_OK");
    }
}

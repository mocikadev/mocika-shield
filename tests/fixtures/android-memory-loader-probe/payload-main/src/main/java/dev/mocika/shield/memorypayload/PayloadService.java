package dev.mocika.shield.memorypayload;

import android.app.Service;
import android.content.Intent;
import android.os.IBinder;
import android.util.Log;

public final class PayloadService extends Service {
    @Override
    public void onCreate() {
        super.onCreate();
        Log.i("MOCIKA_MEMORY_PROBE", "SERVICE_OK");
        stopSelf();
    }

    @Override
    public IBinder onBind(Intent intent) {
        return null;
    }
}

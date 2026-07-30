package dev.mocika.shield.smoke;

import android.app.Service;
import android.content.Intent;
import android.os.IBinder;
import android.util.Log;

public final class SmokeRemoteService extends Service {
    @Override
    public int onStartCommand(Intent intent, int flags, int startId) {
        Log.i("MocikaSmoke", "MOCIKA_SMOKE_REMOTE_SERVICE_OK");
        stopSelf(startId);
        return START_NOT_STICKY;
    }

    @Override
    public IBinder onBind(Intent intent) {
        return null;
    }
}

package dev.mocika.shield.smoke;

import android.app.Application;
import android.os.Process;
import android.util.Log;

import java.io.File;

public final class SmokeApplication extends Application {
    @Override
    public void onCreate() {
        super.onCreate();
        File crashOnce = new File(getFilesDir(), "crash_memory_once");
        if (crashOnce.isFile() && crashOnce.delete()) {
            Log.i("MocikaSmoke", "MOCIKA_SMOKE_CRASH_ONCE");
            Process.killProcess(Process.myPid());
            System.exit(91);
        }
        Log.i("MocikaSmoke", "MOCIKA_SMOKE_APPLICATION_OK");
    }
}

package dev.mocika.shield.memorypayload;

import android.app.Activity;
import android.content.Intent;
import android.os.Bundle;
import android.os.Handler;
import android.os.Looper;
import android.util.Log;
import android.widget.TextView;

public final class PayloadActivity extends Activity {
    static {
        System.loadLibrary("memoryprobe");
    }

    private static native String nativeMarker();

    @Override
    protected void onCreate(Bundle state) {
        super.onCreate(state);
        TextView view = new TextView(this);
        view.setText("MEMORY_DEX_OK");
        setContentView(view);
        startService(new Intent(this, PayloadService.class));
        sendBroadcast(new Intent(this, PayloadReceiver.class));
        Log.i("MOCIKA_MEMORY_PROBE", "ACTIVITY_OK:" + SecondaryMarker.value()
                + ":" + nativeMarker());
        System.gc();
        new Handler(Looper.getMainLooper()).postDelayed(
                () -> Log.i("MOCIKA_MEMORY_PROBE", "DELAYED_OK:" + DelayedMarker.value()),
                500);
    }
}

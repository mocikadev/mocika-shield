package dev.mocika.shield.smoke;

import android.content.BroadcastReceiver;
import android.content.Context;
import android.content.Intent;
import android.util.Log;

public final class SmokeReceiver extends BroadcastReceiver {
    @Override
    public void onReceive(Context context, Intent intent) {
        Log.i("MocikaSmoke", "MOCIKA_SMOKE_RECEIVER_OK");
    }
}

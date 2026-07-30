package dev.mocika.shield.memorypayload;

import android.content.BroadcastReceiver;
import android.content.Context;
import android.content.Intent;
import android.util.Log;

public final class PayloadReceiver extends BroadcastReceiver {
    @Override
    public void onReceive(Context context, Intent intent) {
        Log.i("MOCIKA_MEMORY_PROBE", "RECEIVER_OK");
    }
}

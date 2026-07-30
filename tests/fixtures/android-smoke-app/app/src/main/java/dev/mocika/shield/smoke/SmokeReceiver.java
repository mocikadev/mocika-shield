package dev.mocika.shield.smoke;

import android.content.BroadcastReceiver;
import android.content.Context;
import android.content.Intent;
import android.util.Log;

import java.security.KeyStore;
import java.util.Enumeration;

public final class SmokeReceiver extends BroadcastReceiver {
    public static final String DELETE_RECOVERY_KEYS =
            "dev.mocika.shield.smoke.DELETE_RECOVERY_KEYS";
    private static final String RECOVERY_KEY_PREFIX = "mocika_runtime_recovery_v1_";

    @Override
    public void onReceive(Context context, Intent intent) {
        if (DELETE_RECOVERY_KEYS.equals(intent.getAction())) {
            deleteRecoveryKeys();
            return;
        }
        Log.i("MocikaSmoke", "MOCIKA_SMOKE_RECEIVER_OK");
    }

    static void deleteRecoveryKeys() {
        try {
            KeyStore store = KeyStore.getInstance("AndroidKeyStore");
            store.load(null);
            Enumeration<String> aliases = store.aliases();
            while (aliases.hasMoreElements()) {
                String alias = aliases.nextElement();
                if (alias.startsWith(RECOVERY_KEY_PREFIX)) store.deleteEntry(alias);
            }
            Log.i("MocikaSmoke", "MOCIKA_SMOKE_RECOVERY_KEYS_DELETED");
        } catch (Exception error) {
            throw new IllegalStateException("删除测试恢复密钥失败", error);
        }
    }
}

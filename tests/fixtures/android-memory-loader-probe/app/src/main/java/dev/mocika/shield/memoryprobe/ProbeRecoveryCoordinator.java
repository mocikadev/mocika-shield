package dev.mocika.shield.memoryprobe;

import android.content.Context;
import android.content.SharedPreferences;
import android.security.keystore.KeyGenParameterSpec;
import android.security.keystore.KeyProperties;
import android.util.Log;

import java.nio.charset.StandardCharsets;
import java.security.KeyStore;
import java.security.SecureRandom;
import java.util.Base64;

import javax.crypto.Cipher;
import javax.crypto.KeyGenerator;
import javax.crypto.Mac;
import javax.crypto.SecretKey;
import javax.crypto.spec.GCMParameterSpec;
import javax.crypto.spec.SecretKeySpec;

/** 仅验证跨进程认证回退状态，不进入正式壳。 */
final class ProbeRecoveryCoordinator {
    enum Mode { MEMORY, FILE }

    private static final String TAG = "MOCIKA_MEMORY_PROBE";
    private static final String PREFS = "memory_probe_recovery";
    private static final String KEY_ALIAS = "mocika_memory_probe_recovery_wrap_v1";
    private static final String RECORD = "record";
    private static final String MAC = "mac";
    private static final String WRAPPED_KEY = "wrapped_key";
    private static final String WRAPPED_IV = "wrapped_iv";
    private static final String READY = "memory_ready";
    private static final String MEMORY_PENDING = "memory_pending";
    private static final String FILE_PENDING = "file_pending";
    private static final String FILE_FALLBACK = "file_fallback";
    private static volatile Attempt activeAttempt;
    private static volatile SecretKey softwareHmacKey;

    private ProbeRecoveryCoordinator() {
    }

    static Attempt begin(Context context, String identity) throws Exception {
        synchronized (ProbeRecoveryCoordinator.class) {
            Record previous = read(context);
            Mode mode;
            if (previous == null || !identity.equals(previous.identity)) {
                mode = Mode.MEMORY;
            } else if (MEMORY_PENDING.equals(previous.state)
                    || FILE_FALLBACK.equals(previous.state)) {
                mode = Mode.FILE;
            } else if (FILE_PENDING.equals(previous.state)) {
                throw new SecurityException("MEMORY_PROBE_FILE_FALLBACK_FAILED");
            } else if (READY.equals(previous.state)) {
                mode = Mode.MEMORY;
            } else {
                throw new SecurityException("MEMORY_PROBE_RECOVERY_STATE_INVALID");
            }
            write(context, identity, mode == Mode.MEMORY ? MEMORY_PENDING : FILE_PENDING, true);
            Attempt attempt = new Attempt(identity, mode);
            activeAttempt = attempt;
            Log.i(TAG, "RECOVERY_MODE:" + mode.name());
            return attempt;
        }
    }

    static Mode activeMode() {
        Attempt attempt = activeAttempt;
        if (attempt == null) throw new IllegalStateException("MEMORY_PROBE_RECOVERY_NOT_STARTED");
        return attempt.mode;
    }

    static boolean hasActiveAttempt() {
        return activeAttempt != null;
    }

    static void complete(Context context) throws Exception {
        synchronized (ProbeRecoveryCoordinator.class) {
            Attempt attempt = activeAttempt;
            if (attempt == null) return;
            write(context, attempt.identity,
                    attempt.mode == Mode.MEMORY ? READY : FILE_FALLBACK, false);
            Log.i(TAG, "RECOVERY_COMPLETE:" + attempt.mode.name());
        }
    }

    private static Record read(Context context) throws Exception {
        SharedPreferences preferences = context.getSharedPreferences(PREFS, Context.MODE_PRIVATE);
        String record = preferences.getString(RECORD, null);
        String encodedMac = preferences.getString(MAC, null);
        if (record == null && encodedMac == null) return null;
        if (record == null || encodedMac == null) {
            throw new SecurityException("MEMORY_PROBE_RECOVERY_RECORD_INCOMPLETE");
        }
        byte[] actual = Base64.getDecoder().decode(encodedMac);
        byte[] expected = sign(context, record);
        if (!java.security.MessageDigest.isEqual(actual, expected)) {
            throw new SecurityException("MEMORY_PROBE_RECOVERY_MAC_INVALID");
        }
        String[] fields = record.split("\\|", -1);
        if (fields.length != 3 || !"1".equals(fields[0])) {
            throw new SecurityException("MEMORY_PROBE_RECOVERY_SCHEMA_INVALID");
        }
        return new Record(fields[1], fields[2]);
    }

    private static void write(
            Context context, String identity, String state, boolean durable) throws Exception {
        String record = "1|" + identity + "|" + state;
        String encodedMac = Base64.getEncoder().encodeToString(sign(context, record));
        SharedPreferences.Editor editor = context.getSharedPreferences(PREFS, Context.MODE_PRIVATE)
                .edit().putString(RECORD, record).putString(MAC, encodedMac);
        if (durable) {
            if (!editor.commit()) {
                throw new IllegalStateException("MEMORY_PROBE_RECOVERY_COMMIT_FAILED");
            }
        } else {
            editor.apply();
        }
    }

    private static byte[] sign(Context context, String record) throws Exception {
        Mac mac = Mac.getInstance("HmacSHA256");
        mac.init(key(context));
        return mac.doFinal(record.getBytes(StandardCharsets.UTF_8));
    }

    private static SecretKey key(Context context) throws Exception {
        SecretKey cached = softwareHmacKey;
        if (cached != null) return cached;
        synchronized (ProbeRecoveryCoordinator.class) {
            if (softwareHmacKey != null) return softwareHmacKey;
            SharedPreferences preferences = context.getSharedPreferences(PREFS, Context.MODE_PRIVATE);
            String wrapped = preferences.getString(WRAPPED_KEY, null);
            String encodedIv = preferences.getString(WRAPPED_IV, null);
            byte[] raw;
            if (wrapped == null && encodedIv == null) {
                if (preferences.contains(RECORD) || preferences.contains(MAC)) {
                    throw new SecurityException("MEMORY_PROBE_RECOVERY_KEY_MISSING");
                }
                raw = new byte[32];
                new SecureRandom().nextBytes(raw);
                Cipher cipher = Cipher.getInstance("AES/GCM/NoPadding");
                cipher.init(Cipher.ENCRYPT_MODE, wrappingKey());
                byte[] ciphertext = cipher.doFinal(raw);
                boolean committed = preferences.edit()
                        .putString(WRAPPED_KEY, Base64.getEncoder().encodeToString(ciphertext))
                        .putString(WRAPPED_IV, Base64.getEncoder().encodeToString(cipher.getIV()))
                        .commit();
                if (!committed) throw new IllegalStateException("MEMORY_PROBE_RECOVERY_KEY_COMMIT_FAILED");
            } else {
                if (wrapped == null || encodedIv == null) {
                    throw new SecurityException("MEMORY_PROBE_RECOVERY_KEY_INCOMPLETE");
                }
                Cipher cipher = Cipher.getInstance("AES/GCM/NoPadding");
                cipher.init(Cipher.DECRYPT_MODE, wrappingKey(), new GCMParameterSpec(
                        128, Base64.getDecoder().decode(encodedIv)));
                raw = cipher.doFinal(Base64.getDecoder().decode(wrapped));
            }
            softwareHmacKey = new SecretKeySpec(raw, "HmacSHA256");
            java.util.Arrays.fill(raw, (byte) 0);
            return softwareHmacKey;
        }
    }

    private static SecretKey wrappingKey() throws Exception {
        KeyStore store = KeyStore.getInstance("AndroidKeyStore");
        store.load(null);
        java.security.Key existing = store.getKey(KEY_ALIAS, null);
        if (existing instanceof SecretKey) return (SecretKey) existing;
        KeyGenerator generator = KeyGenerator.getInstance(
                KeyProperties.KEY_ALGORITHM_AES, "AndroidKeyStore");
        generator.init(new KeyGenParameterSpec.Builder(
                KEY_ALIAS, KeyProperties.PURPOSE_ENCRYPT | KeyProperties.PURPOSE_DECRYPT)
                .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
                .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
                .setKeySize(256).build());
        return generator.generateKey();
    }

    static final class Attempt {
        final String identity;
        final Mode mode;

        Attempt(String identity, Mode mode) {
            this.identity = identity;
            this.mode = mode;
        }
    }

    private static final class Record {
        final String identity;
        final String state;

        Record(String identity, String state) {
            this.identity = identity;
            this.state = state;
        }
    }
}

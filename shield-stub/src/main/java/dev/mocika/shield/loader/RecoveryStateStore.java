package dev.mocika.shield.loader;

import android.annotation.TargetApi;
import android.content.Context;
import android.security.keystore.KeyGenParameterSpec;
import android.security.keystore.KeyProperties;

import java.io.BufferedInputStream;
import java.io.BufferedOutputStream;
import java.io.DataInputStream;
import java.io.DataOutputStream;
import java.io.EOFException;
import java.io.File;
import java.io.FileInputStream;
import java.io.FileOutputStream;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.StandardCopyOption;
import java.security.KeyStore;
import java.security.MessageDigest;
import java.security.SecureRandom;

import javax.crypto.Cipher;
import javax.crypto.KeyGenerator;
import javax.crypto.Mac;
import javax.crypto.SecretKey;
import javax.crypto.spec.GCMParameterSpec;
import javax.crypto.spec.SecretKeySpec;

/** 独占每进程认证状态文件及其 Android Keystore 包装材料。 */
@TargetApi(29)
final class RecoveryStateStore {
    private static final int MAGIC = 0x4d535252;
    private static final int SCHEMA = 1;
    private static final int MAX_BLOB = 4096;
    private static final String KEY_ALIAS_PREFIX = "mocika_runtime_recovery_v1_";

    private final File stateFile;
    private final String processName;
    private final String processId;
    private SecretKey hmacKey;
    private byte[] wrappedKey;
    private byte[] wrappedIv;

    RecoveryStateStore(Context context, String processName) throws Exception {
        this.processName = processName;
        this.processId = hex(MessageDigest.getInstance("SHA-256").digest(
                processName.getBytes(StandardCharsets.UTF_8))).substring(0, 24);
        File directory = new File(context.getNoBackupFilesDir(), "runtime_state");
        if (!directory.isDirectory() && !directory.mkdirs()) {
            throw new IllegalStateException("R03");
        }
        stateFile = new File(directory, processId + ".bin");
    }

    Record read() throws Exception {
        if (!stateFile.exists()) return null;
        Stored stored;
        try (DataInputStream input = new DataInputStream(
                new BufferedInputStream(new FileInputStream(stateFile)))) {
            if (input.readInt() != MAGIC || input.readInt() != SCHEMA) {
                throw new SecurityException("R04");
            }
            String storedProcess = input.readUTF();
            String identity = input.readUTF();
            String state = input.readUTF();
            byte[] iv = readBlob(input);
            byte[] key = readBlob(input);
            byte[] mac = readBlob(input);
            if (input.read() != -1 || !processName.equals(storedProcess)) {
                throw new SecurityException("R05");
            }
            stored = new Stored(identity, state, iv, key, mac);
        } catch (EOFException error) {
            throw new SecurityException("R06", error);
        }
        SecretKey key = unwrap(stored.wrappedKey, stored.iv);
        byte[] expected = sign(key, stored.identity, stored.state);
        if (!MessageDigest.isEqual(expected, stored.mac)) throw new SecurityException("R07");
        hmacKey = key;
        wrappedKey = stored.wrappedKey;
        wrappedIv = stored.iv;
        return new Record(stored.identity, stored.state);
    }

    void write(String identity, String state) throws Exception {
        ensureKey();
        byte[] mac = sign(hmacKey, identity, state);
        File temporary = new File(stateFile.getParentFile(), stateFile.getName() + ".tmp");
        try (FileOutputStream file = new FileOutputStream(temporary);
             DataOutputStream output = new DataOutputStream(new BufferedOutputStream(file))) {
            output.writeInt(MAGIC);
            output.writeInt(SCHEMA);
            output.writeUTF(processName);
            output.writeUTF(identity);
            output.writeUTF(state);
            writeBlob(output, wrappedIv);
            writeBlob(output, wrappedKey);
            writeBlob(output, mac);
            output.flush();
            file.getFD().sync();
        }
        try {
            Files.move(temporary.toPath(), stateFile.toPath(),
                    StandardCopyOption.ATOMIC_MOVE, StandardCopyOption.REPLACE_EXISTING);
        } catch (Exception error) {
            throw new SecurityException("R09", error);
        }
    }

    private void ensureKey() throws Exception {
        if (hmacKey != null) return;
        if (stateFile.exists()) throw new SecurityException("R10");
        byte[] raw = new byte[32];
        new SecureRandom().nextBytes(raw);
        try {
            Cipher cipher = Cipher.getInstance("AES/GCM/NoPadding");
            cipher.init(Cipher.ENCRYPT_MODE, wrappingKey());
            wrappedKey = cipher.doFinal(raw);
            wrappedIv = cipher.getIV();
            hmacKey = new SecretKeySpec(raw, "HmacSHA256");
        } finally {
            java.util.Arrays.fill(raw, (byte) 0);
        }
    }

    private SecretKey unwrap(byte[] key, byte[] iv) throws Exception {
        try {
            Cipher cipher = Cipher.getInstance("AES/GCM/NoPadding");
            cipher.init(Cipher.DECRYPT_MODE, wrappingKey(), new GCMParameterSpec(128, iv));
            byte[] raw = cipher.doFinal(key);
            try {
                return new SecretKeySpec(raw, "HmacSHA256");
            } finally {
                java.util.Arrays.fill(raw, (byte) 0);
            }
        } catch (Exception error) {
            throw new SecurityException("R11", error);
        }
    }

    private SecretKey wrappingKey() throws Exception {
        String alias = KEY_ALIAS_PREFIX + processId;
        KeyStore store = KeyStore.getInstance("AndroidKeyStore");
        store.load(null);
        java.security.Key existing = store.getKey(alias, null);
        if (existing instanceof SecretKey) return (SecretKey) existing;
        KeyGenerator generator = KeyGenerator.getInstance(
                KeyProperties.KEY_ALGORITHM_AES, "AndroidKeyStore");
        generator.init(new KeyGenParameterSpec.Builder(
                alias, KeyProperties.PURPOSE_ENCRYPT | KeyProperties.PURPOSE_DECRYPT)
                .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
                .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
                .setKeySize(256)
                .build());
        return generator.generateKey();
    }

    private byte[] sign(SecretKey key, String identity, String state) throws Exception {
        Mac mac = Mac.getInstance("HmacSHA256");
        mac.init(key);
        mac.update((byte) SCHEMA);
        update(mac, processName);
        update(mac, identity);
        update(mac, state);
        return mac.doFinal();
    }

    private static void update(Mac mac, String value) {
        byte[] bytes = value.getBytes(StandardCharsets.UTF_8);
        mac.update((byte) (bytes.length >>> 24));
        mac.update((byte) (bytes.length >>> 16));
        mac.update((byte) (bytes.length >>> 8));
        mac.update((byte) bytes.length);
        mac.update(bytes);
    }

    private static byte[] readBlob(DataInputStream input) throws Exception {
        int length = input.readInt();
        if (length <= 0 || length > MAX_BLOB) throw new SecurityException("R12");
        byte[] value = new byte[length];
        input.readFully(value);
        return value;
    }

    private static void writeBlob(DataOutputStream output, byte[] value) throws Exception {
        if (value == null || value.length == 0 || value.length > MAX_BLOB) {
            throw new SecurityException("R13");
        }
        output.writeInt(value.length);
        output.write(value);
    }

    private static String hex(byte[] bytes) {
        StringBuilder result = new StringBuilder(bytes.length * 2);
        for (byte value : bytes) result.append(String.format("%02x", value & 0xff));
        return result.toString();
    }

    static final class Record {
        final String identity;
        final String state;

        Record(String identity, String state) {
            this.identity = identity;
            this.state = state;
        }
    }

    private static final class Stored {
        final String identity;
        final String state;
        final byte[] iv;
        final byte[] wrappedKey;
        final byte[] mac;

        Stored(String identity, String state, byte[] iv, byte[] wrappedKey, byte[] mac) {
            this.identity = identity;
            this.state = state;
            this.iv = iv;
            this.wrappedKey = wrappedKey;
            this.mac = mac;
        }
    }
}

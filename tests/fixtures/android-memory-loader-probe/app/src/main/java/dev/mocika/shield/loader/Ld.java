package dev.mocika.shield.loader;

import android.content.Context;
import android.content.pm.PackageInfo;

import java.security.MessageDigest;

/** 仅为隔离探针提供正式 Stub Native 所需的最小 JNI 契约。 */
public final class Ld {
    static {
        System.loadLibrary("mocikashield");
    }

    private Ld() {}

    private static native boolean p(
            ClassLoader classLoader, String[] dexPaths, String optimizedDirectory);

    private static native byte[][] q(Context context, byte[] dexData);

    private static native int r();

    public static byte[][] decrypt(Context context, byte[] dexData) {
        byte[][] result = q(context, dexData);
        if (result == null || result.length == 0) {
            throw new IllegalStateException("MEMORY_PROBE_DECRYPT_EMPTY");
        }
        return result;
    }

    static String getSignatureSha256(Context context) throws Exception {
        PackageInfo packageInfo = context.getPackageManager().getPackageInfo(
                context.getPackageName(), android.content.pm.PackageManager.GET_SIGNING_CERTIFICATES);
        if (packageInfo.signingInfo == null || packageInfo.signingInfo.hasMultipleSigners()) {
            throw new SecurityException("MEMORY_PROBE_SIGNATURE_INVALID");
        }
        android.content.pm.Signature[] signatures = packageInfo.signingInfo.getApkContentsSigners();
        if (signatures == null || signatures.length != 1 || signatures[0] == null) {
            throw new SecurityException("MEMORY_PROBE_SIGNATURE_MISSING");
        }
        byte[] digest = MessageDigest.getInstance("SHA-256").digest(signatures[0].toByteArray());
        StringBuilder result = new StringBuilder(64);
        for (byte value : digest) {
            result.append(String.format("%02X", value & 0xFF));
        }
        return result.toString();
    }
}

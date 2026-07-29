package dev.mocika.shield.loader;

import android.content.Context;
import android.content.pm.PackageInfo;
import android.content.pm.PackageManager;

import java.io.ByteArrayOutputStream;
import java.io.File;
import java.io.InputStream;
import java.util.List;

public class Ld {

    private static final int DEX_READ_BUFFER_SIZE = 8192;
    private static final int MAX_DEX_SIZE_BYTES = 256 * 1024 * 1024; // 256 MiB 合理上限

    static {
        System.loadLibrary("mocikanativeslot");
    }

    /** DEX 注入：通过 JNI 将解密后的 DEX 插入 PathClassLoader，成功返回 true，失败降级到 Java 反射。 */
    static native boolean p(ClassLoader classLoader, String[] dexPaths, String optDirPath);

    static String getSignatureSha256(Context ctx) throws Exception {
        android.content.pm.PackageManager pm = ctx.getPackageManager();
        String pkg = ctx.getPackageName();
        android.content.pm.Signature[] signatures;
        if (android.os.Build.VERSION.SDK_INT >= 28) {
            PackageInfo pi = pm.getPackageInfo(pkg, android.content.pm.PackageManager.GET_SIGNING_CERTIFICATES);
            if (pi.signingInfo == null || pi.signingInfo.hasMultipleSigners()) {
                throw new SecurityException("当前仅支持单签名 APK");
            }
            signatures = pi.signingInfo.getApkContentsSigners();
        } else {
            @SuppressWarnings("deprecation")
            PackageInfo pi = pm.getPackageInfo(pkg, android.content.pm.PackageManager.GET_SIGNATURES);
            signatures = pi.signatures;
        }
        if (signatures == null || signatures.length != 1 || signatures[0] == null) {
            throw new SecurityException("无法确定唯一的 APK 签名证书");
        }
        // Signature.toByteArray() 返回 X.509 证书 DER，与 apksigner 的 certificate digest 口径一致。
        byte[] certBytes = signatures[0].toByteArray();
        java.security.MessageDigest md = java.security.MessageDigest.getInstance("SHA-256");
        byte[] digest = md.digest(certBytes);
        StringBuilder sb = new StringBuilder(64);
        for (byte b : digest) {
            sb.append(String.format("%02X", b & 0xFF));
        }
        return sb.toString();
    }

    /** 从 classes.dex 末尾提取 MSHD 封装的加密 payload，解密解压后返回各 DEX 字节数组。 */
    private static native byte[][] q(Context ctx, byte[] dexData);

    /** 每次启动的环境安全检查；返回 true 表示当前环境不安全。 */
    static native boolean r();

    /**
     * 解密 app.bin 并将各 DEX 文件落地到私有目录。
     * 以 versionCode 为缓存键：同一版本只解密解压一次，升级后自动失效并清理旧缓存。
     * 返回落地后的 DEX 文件列表（顺序与原始 DEX 顺序一致）。
     */
    static byte[][] decryptDexBytes(Context ctx) throws Exception {
        byte[] dexBytes = readClassesDexFromApk(ctx);
        byte[][] dexes = q(ctx, dexBytes);
        if (dexes == null || dexes.length == 0)
            throw new RuntimeException("q 返回空结果");
        return dexes;
    }

    public static List<File> extractDexFiles(Context ctx) throws Exception {
        return DexCache.load(ctx);
    }

    private static byte[] readClassesDexFromApk(Context ctx) throws Exception {
        String apkPath = ctx.getApplicationInfo().sourceDir;
        try (java.util.zip.ZipFile zip = new java.util.zip.ZipFile(apkPath)) {
            java.util.zip.ZipEntry entry = zip.getEntry("classes.dex");
            if (entry == null) throw new RuntimeException("APK 中未找到 classes.dex");
            long entrySize = entry.getSize();
            if (entrySize > MAX_DEX_SIZE_BYTES) {
                throw new RuntimeException("classes.dex 大小 " + entrySize + " 超过上限 " + MAX_DEX_SIZE_BYTES);
            }
            int initCapacity = entrySize > 0 ? (int) entrySize : DEX_READ_BUFFER_SIZE * 16;
            try (InputStream is = zip.getInputStream(entry)) {
                ByteArrayOutputStream bos = new ByteArrayOutputStream(initCapacity);
                byte[] buf = new byte[DEX_READ_BUFFER_SIZE];
                int read;
                while ((read = is.read(buf)) != -1) {
                    bos.write(buf, 0, read);
                    if (bos.size() > MAX_DEX_SIZE_BYTES) {
                        throw new RuntimeException("classes.dex 读取超过上限 " + MAX_DEX_SIZE_BYTES + " 字节");
                    }
                }
                return bos.toByteArray();
            }
        }
    }
}

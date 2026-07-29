package dev.mocika.shield.loader;

import android.content.Context;
import android.content.pm.ApplicationInfo;
import android.content.pm.PackageInfo;
import android.content.pm.PackageManager;
import android.os.Bundle;
import android.util.Log;

import java.io.File;
import java.io.FileInputStream;
import java.io.FileOutputStream;
import java.nio.ByteBuffer;
import java.nio.ByteOrder;
import java.security.MessageDigest;
import java.util.ArrayList;
import java.util.List;

final class DexCache {
    private static final String TAG = "cx";
    private static final String DONE = ".done";
    private static final String KEY_SCHEMA = "dev.mocika.shield.CACHE_SCHEMA";
    private static final String KEY_COUNT = "dev.mocika.shield.CACHE_DEX_COUNT";
    private static final String KEY_ROOT = "dev.mocika.shield.CACHE_ROOT_SHA256";
    private static final int SCHEMA = 1;
    private static final int BUFFER_SIZE = 8192;

    private DexCache() {}

    static List<File> load(Context context) throws Exception {
        Identity identity = readIdentity(context);
        long versionCode = getVersionCode(context);
        File baseDir = context.getDir("app_dex", Context.MODE_PRIVATE);
        String cacheName = "v" + versionCode + "-" + identity.root.substring(0, 16);
        File cacheDir = new File(baseDir, cacheName);

        if (cacheDir.exists()) {
            if (validate(cacheDir, identity)) return files(cacheDir, identity.count);
            removeInvalidCache(cacheDir);
        }

        byte[][] dexes = Ld.decryptDexBytes(context);
        if (dexes == null || dexes.length != identity.count) {
            throw new SecurityException("解密 DEX 数量与签名清单不一致");
        }

        File temporary = new File(baseDir, cacheName + ".tmp");
        if (temporary.exists() && !deleteRecursive(temporary)) {
            throw new SecurityException("临时缓存清理失败");
        }
        if (!temporary.mkdirs()) throw new SecurityException("临时缓存目录创建失败");

        for (int i = 0; i < dexes.length; i++) {
            File output = new File(temporary, fileName(i));
            try (FileOutputStream stream = new FileOutputStream(output)) {
                if (!output.setReadOnly()) throw new SecurityException("DEX 设置只读失败");
                stream.write(dexes[i]);
                stream.flush();
                stream.getFD().sync();
            }
        }
        if (!validateDexFiles(temporary, identity)) {
            deleteRecursive(temporary);
            throw new SecurityException("解密 DEX 与签名清单不一致");
        }
        File done = new File(temporary, DONE);
        if (!done.createNewFile() || !done.setReadOnly()) {
            deleteRecursive(temporary);
            throw new SecurityException("缓存完成标记创建失败");
        }
        if (cacheDir.exists() && !deleteRecursive(cacheDir)) {
            deleteRecursive(temporary);
            throw new SecurityException("目标缓存清理失败");
        }
        if (!temporary.renameTo(cacheDir)) {
            deleteRecursive(temporary);
            throw new SecurityException("缓存目录原子替换失败");
        }
        cleanOldCaches(baseDir, cacheName);
        return files(cacheDir, identity.count);
    }

    static boolean validate(File cacheDir, Identity identity) throws Exception {
        if (!cacheDir.isDirectory() || !isDirectChild(cacheDir.getParentFile(), cacheDir)) return false;
        File[] children = cacheDir.listFiles();
        if (children == null || children.length != identity.count + 1) return false;
        File done = new File(cacheDir, DONE);
        return done.isFile() && validateDexFiles(cacheDir, identity);
    }

    private static boolean validateDexFiles(File directory, Identity identity) throws Exception {
        for (int i = 0; i < identity.count; i++) {
            File file = new File(directory, fileName(i));
            if (!file.isFile() || !isDirectChild(directory, file) || file.canWrite()) return false;
        }
        return calculateRoot(directory, identity.schema, identity.count).equals(identity.root);
    }

    static String calculateRoot(File directory, int schema, int count) throws Exception {
        MessageDigest root = MessageDigest.getInstance("SHA-256");
        updateInt(root, schema);
        updateInt(root, count);
        for (int i = 0; i < count; i++) {
            File file = new File(directory, fileName(i));
            String originalName = i == 0 ? "classes.dex" : "classes" + (i + 1) + ".dex";
            updateInt(root, i + 1);
            byte[] name = originalName.getBytes("UTF-8");
            updateInt(root, name.length);
            root.update(name);
            updateLong(root, file.length());
            root.update(digestFile(file));
        }
        return hex(root.digest());
    }

    private static byte[] digestFile(File file) throws Exception {
        MessageDigest digest = MessageDigest.getInstance("SHA-256");
        try (FileInputStream stream = new FileInputStream(file)) {
            byte[] buffer = new byte[BUFFER_SIZE];
            int read;
            while ((read = stream.read(buffer)) != -1) digest.update(buffer, 0, read);
        }
        return digest.digest();
    }

    private static Identity readIdentity(Context context) throws Exception {
        ApplicationInfo info = context.getPackageManager().getApplicationInfo(
                context.getPackageName(), PackageManager.GET_META_DATA);
        Bundle metadata = info.metaData;
        if (metadata == null) throw new SecurityException("缺少缓存身份");
        int schema = metadata.getInt(KEY_SCHEMA, -1);
        int count = metadata.getInt(KEY_COUNT, -1);
        String root = metadata.getString(KEY_ROOT);
        if (schema != SCHEMA || count <= 0 || root == null || !root.matches("[0-9a-f]{64}")) {
            throw new SecurityException("缓存身份非法");
        }
        return new Identity(schema, count, root);
    }

    private static List<File> files(File directory, int count) {
        List<File> result = new ArrayList<>(count);
        for (int i = 0; i < count; i++) result.add(new File(directory, fileName(i)));
        return result;
    }

    private static String fileName(int zeroBasedIndex) {
        return "c" + (zeroBasedIndex + 1) + ".dex";
    }

    private static boolean isDirectChild(File parent, File child) throws Exception {
        if (parent == null) return false;
        File canonical = child.getCanonicalFile();
        return canonical.getParentFile().equals(parent.getCanonicalFile())
                && canonical.getName().equals(child.getName());
    }

    private static void cleanOldCaches(File baseDir, String keepName) {
        File[] children = baseDir.listFiles();
        if (children == null) return;
        for (File child : children) {
            if (!child.getName().equals(keepName) && !deleteRecursive(child)) {
                Log.w(TAG, "旧缓存清理失败: " + child.getName());
            }
        }
    }

    private static boolean deleteRecursive(File file) {
        boolean success = true;
        if (file.isDirectory()) {
            File[] children = file.listFiles();
            if (children != null) for (File child : children) success &= deleteRecursive(child);
        }
        return file.delete() && success;
    }

    static void removeInvalidCache(File cacheDir) {
        if (!deleteRecursive(cacheDir)) throw new SecurityException("无效缓存清理失败");
    }

    private static long getVersionCode(Context context) throws Exception {
        PackageInfo info = context.getPackageManager().getPackageInfo(context.getPackageName(), 0);
        return android.os.Build.VERSION.SDK_INT >= 28 ? info.getLongVersionCode() : info.versionCode;
    }

    private static void updateInt(MessageDigest digest, int value) {
        digest.update(ByteBuffer.allocate(4).order(ByteOrder.LITTLE_ENDIAN).putInt(value).array());
    }

    private static void updateLong(MessageDigest digest, long value) {
        digest.update(ByteBuffer.allocate(8).order(ByteOrder.LITTLE_ENDIAN).putLong(value).array());
    }

    private static String hex(byte[] bytes) {
        StringBuilder output = new StringBuilder(bytes.length * 2);
        for (byte value : bytes) output.append(String.format("%02x", value & 0xff));
        return output.toString();
    }

    static final class Identity {
        final int schema;
        final int count;
        final String root;

        Identity(int schema, int count, String root) {
            this.schema = schema;
            this.count = count;
            this.root = root;
        }
    }
}

package dev.mocika.shield.memoryprobe;

import android.content.Context;
import android.content.pm.ApplicationInfo;
import android.os.Build;

import java.io.ByteArrayOutputStream;
import java.io.InputStream;
import java.nio.ByteBuffer;
import java.security.MessageDigest;
import java.util.zip.ZipEntry;
import java.util.zip.ZipFile;

import dalvik.system.InMemoryDexClassLoader;
import dalvik.system.DexClassLoader;
import dev.mocika.shield.loader.Ld;

/** 仅负责从探针 APK 读取内存 DEX 并创建业务加载器。 */
final class MemoryPayloadLoader {
    private static final String[] PAYLOAD_ENTRIES = {
            "assets/payload-main.dex",
            "assets/payload-secondary.dex"
    };
    private static final String PROTECTED_PAYLOAD_ENTRY = "assets/protected-payload.dex";

    private MemoryPayloadLoader() {}

    static ClassLoader create(Context context, ClassLoader parent) throws Exception {
        ApplicationInfo info = context.getApplicationInfo();
        MemoryProbeMetrics.configure(context);
        try (ZipFile apk = new ZipFile(info.sourceDir)) {
            ZipEntry protectedPayload = apk.getEntry(PROTECTED_PAYLOAD_ENTRY);
            if (protectedPayload != null) {
                MemoryProbeMetrics.snapshot("before_payload_read");
                byte[] encrypted = readBytes(apk, protectedPayload);
                String identity = hex(MessageDigest.getInstance("SHA-256").digest(encrypted));
                ProbeRecoveryCoordinator.Attempt attempt =
                        ProbeRecoveryCoordinator.begin(context, identity);
                MemoryProbeMetrics.trackEncrypted(encrypted);
                MemoryProbeMetrics.snapshot("after_payload_read");
                byte[][] decrypted = Ld.decrypt(context, encrypted);
                MemoryProbeMetrics.trackDecrypted(decrypted);
                MemoryProbeMetrics.snapshot("after_decrypt");
                android.util.Log.i("MOCIKA_MEMORY_PROBE", "DEXB_NATIVE_DECRYPT_OK");
                ClassLoader loader;
                if (attempt.mode == ProbeRecoveryCoordinator.Mode.MEMORY) {
                    ByteBuffer[] payload = new ByteBuffer[decrypted.length];
                    for (int index = 0; index < decrypted.length; index++) {
                        payload[index] = directBuffer(decrypted[index]);
                    }
                    MemoryProbeMetrics.trackDirectBuffers(payload);
                    MemoryProbeMetrics.snapshot("after_direct_copy");
                    loader = new InMemoryDexClassLoader(
                            payload, buildNativeSearchPath(info), parent);
                } else {
                    loader = createFileLoader(
                            context, decrypted, buildNativeSearchPath(info), parent);
                }
                MemoryProbeMetrics.snapshot("after_loader_create");
                return loader;
            }
            ByteBuffer[] payload = new ByteBuffer[PAYLOAD_ENTRIES.length];
            for (int index = 0; index < PAYLOAD_ENTRIES.length; index++) {
                payload[index] = readDirectBuffer(apk, PAYLOAD_ENTRIES[index]);
            }
            return new InMemoryDexClassLoader(payload, buildNativeSearchPath(info), parent);
        }
    }

    private static ClassLoader createFileLoader(
            Context context, byte[][] dexes, String nativePath, ClassLoader parent) throws Exception {
        ApplicationInfo info = context.getPackageManager().getApplicationInfo(
                context.getPackageName(), android.content.pm.PackageManager.GET_META_DATA);
        if (info.metaData != null
                && info.metaData.getBoolean("PROBE_FAIL_FILE_START", false)) {
            throw new IllegalStateException("MEMORY_PROBE_INJECTED_FILE_FAILURE");
        }
        java.io.File directory = new java.io.File(context.getCodeCacheDir(), "memory-probe-fallback");
        if (!directory.exists() && !directory.mkdirs()) {
            throw new IllegalStateException("MEMORY_PROBE_FALLBACK_DIR_FAILED");
        }
        StringBuilder paths = new StringBuilder();
        for (int index = 0; index < dexes.length; index++) {
            java.io.File temporary = new java.io.File(directory, "c" + index + ".dex.tmp");
            java.io.File output = new java.io.File(directory, "c" + index + ".dex");
            try (java.io.FileOutputStream stream = new java.io.FileOutputStream(temporary)) {
                stream.write(dexes[index]);
                stream.getFD().sync();
            }
            java.nio.file.Files.move(
                    temporary.toPath(), output.toPath(),
                    java.nio.file.StandardCopyOption.ATOMIC_MOVE,
                    java.nio.file.StandardCopyOption.REPLACE_EXISTING);
            if (!output.setReadOnly()) {
                throw new IllegalStateException("MEMORY_PROBE_FALLBACK_COMMIT_FAILED");
            }
            if (paths.length() > 0) paths.append(java.io.File.pathSeparatorChar);
            paths.append(output.getAbsolutePath());
        }
        android.util.Log.i("MOCIKA_MEMORY_PROBE", "RECOVERY_FILE_LOADER_READY");
        return new DexClassLoader(paths.toString(), directory.getAbsolutePath(),
                nativePath, parent);
    }

    private static String hex(byte[] bytes) {
        StringBuilder result = new StringBuilder(bytes.length * 2);
        for (byte value : bytes) result.append(String.format("%02x", value & 0xff));
        return result.toString();
    }

    private static ByteBuffer readDirectBuffer(ZipFile apk, String name) throws Exception {
        ZipEntry entry = apk.getEntry(name);
        if (entry == null) {
            throw new IllegalStateException("MEMORY_PROBE_ASSET_MISSING:" + name);
        }
        return directBuffer(readBytes(apk, entry));
    }

    private static byte[] readBytes(ZipFile apk, ZipEntry entry) throws Exception {
        long declaredSize = entry.getSize();
        if (declaredSize >= 0 && declaredSize <= Integer.MAX_VALUE) {
            byte[] result = new byte[(int) declaredSize];
            try (InputStream input = apk.getInputStream(entry)) {
                int offset = 0;
                while (offset < result.length) {
                    int count = input.read(result, offset, result.length - offset);
                    if (count < 0) {
                        throw new IllegalStateException(
                                "MEMORY_PROBE_ASSET_TRUNCATED:" + entry.getName());
                    }
                    offset += count;
                }
                if (input.read() != -1) {
                    throw new IllegalStateException(
                            "MEMORY_PROBE_ASSET_SIZE_MISMATCH:" + entry.getName());
                }
            }
            return result;
        }
        try (InputStream input = apk.getInputStream(entry);
             ByteArrayOutputStream output = new ByteArrayOutputStream()) {
            byte[] chunk = new byte[16 * 1024];
            int count;
            while ((count = input.read(chunk)) >= 0) {
                output.write(chunk, 0, count);
            }
            return output.toByteArray();
        }
    }

    private static ByteBuffer directBuffer(byte[] bytes) {
        ByteBuffer buffer = ByteBuffer.allocateDirect(bytes.length);
        buffer.put(bytes);
        buffer.flip();
        return buffer;
    }

    private static String buildNativeSearchPath(ApplicationInfo info) {
        StringBuilder paths = new StringBuilder();
        for (String abi : Build.SUPPORTED_ABIS) {
            if (paths.length() > 0) {
                paths.append(java.io.File.pathSeparatorChar);
            }
            paths.append(info.sourceDir).append("!/lib/").append(abi);
        }
        if (info.nativeLibraryDir != null && !info.nativeLibraryDir.isEmpty()) {
            paths.append(java.io.File.pathSeparatorChar).append(info.nativeLibraryDir);
        }
        return paths.toString();
    }
}

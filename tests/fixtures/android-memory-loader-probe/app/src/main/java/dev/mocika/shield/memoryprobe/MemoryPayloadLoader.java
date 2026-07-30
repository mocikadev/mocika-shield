package dev.mocika.shield.memoryprobe;

import android.content.Context;
import android.content.pm.ApplicationInfo;
import android.os.Build;

import java.io.ByteArrayOutputStream;
import java.io.InputStream;
import java.nio.ByteBuffer;
import java.util.zip.ZipEntry;
import java.util.zip.ZipFile;

import dalvik.system.InMemoryDexClassLoader;
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
        try (ZipFile apk = new ZipFile(info.sourceDir)) {
            ZipEntry protectedPayload = apk.getEntry(PROTECTED_PAYLOAD_ENTRY);
            if (protectedPayload != null) {
                byte[][] decrypted = Ld.decrypt(context, readBytes(apk, protectedPayload));
                ByteBuffer[] payload = new ByteBuffer[decrypted.length];
                for (int index = 0; index < decrypted.length; index++) {
                    payload[index] = directBuffer(decrypted[index]);
                }
                android.util.Log.i("MOCIKA_MEMORY_PROBE", "DEXB_NATIVE_DECRYPT_OK");
                return new InMemoryDexClassLoader(payload, buildNativeSearchPath(info), parent);
            }
            ByteBuffer[] payload = new ByteBuffer[PAYLOAD_ENTRIES.length];
            for (int index = 0; index < PAYLOAD_ENTRIES.length; index++) {
                payload[index] = readDirectBuffer(apk, PAYLOAD_ENTRIES[index]);
            }
            return new InMemoryDexClassLoader(payload, buildNativeSearchPath(info), parent);
        }
    }

    private static ByteBuffer readDirectBuffer(ZipFile apk, String name) throws Exception {
        ZipEntry entry = apk.getEntry(name);
        if (entry == null) {
            throw new IllegalStateException("MEMORY_PROBE_ASSET_MISSING:" + name);
        }
        return directBuffer(readBytes(apk, entry));
    }

    private static byte[] readBytes(ZipFile apk, ZipEntry entry) throws Exception {
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

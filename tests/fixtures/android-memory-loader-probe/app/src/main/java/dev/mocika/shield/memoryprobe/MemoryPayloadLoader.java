package dev.mocika.shield.memoryprobe;

import android.content.pm.ApplicationInfo;
import android.os.Build;

import java.io.ByteArrayOutputStream;
import java.io.InputStream;
import java.nio.ByteBuffer;
import java.util.zip.ZipEntry;
import java.util.zip.ZipFile;

import dalvik.system.InMemoryDexClassLoader;

/** 仅负责从探针 APK 读取内存 DEX 并创建业务加载器。 */
final class MemoryPayloadLoader {
    private static final String[] PAYLOAD_ENTRIES = {
            "assets/payload-main.dex",
            "assets/payload-secondary.dex"
    };

    private MemoryPayloadLoader() {}

    static ClassLoader create(ApplicationInfo info, ClassLoader parent) throws Exception {
        ByteBuffer[] payload = new ByteBuffer[PAYLOAD_ENTRIES.length];
        try (ZipFile apk = new ZipFile(info.sourceDir)) {
            for (int index = 0; index < PAYLOAD_ENTRIES.length; index++) {
                payload[index] = readDirectBuffer(apk, PAYLOAD_ENTRIES[index]);
            }
        }
        return new InMemoryDexClassLoader(payload, buildNativeSearchPath(info), parent);
    }

    private static ByteBuffer readDirectBuffer(ZipFile apk, String name) throws Exception {
        ZipEntry entry = apk.getEntry(name);
        if (entry == null) {
            throw new IllegalStateException("MEMORY_PROBE_ASSET_MISSING:" + name);
        }
        try (InputStream input = apk.getInputStream(entry);
             ByteArrayOutputStream output = new ByteArrayOutputStream()) {
            byte[] chunk = new byte[16 * 1024];
            int count;
            while ((count = input.read(chunk)) >= 0) {
                output.write(chunk, 0, count);
            }
            byte[] bytes = output.toByteArray();
            ByteBuffer buffer = ByteBuffer.allocateDirect(bytes.length);
            buffer.put(bytes);
            buffer.flip();
            return buffer;
        }
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

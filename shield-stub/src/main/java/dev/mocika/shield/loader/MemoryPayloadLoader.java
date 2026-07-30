package dev.mocika.shield.loader;

import android.annotation.TargetApi;
import android.content.Context;
import android.content.pm.ApplicationInfo;
import android.os.Build;

import java.io.File;
import java.nio.ByteBuffer;
import java.util.LinkedHashSet;
import java.util.Set;

import dalvik.system.InMemoryDexClassLoader;

/** 只负责把正式 DEXB 解密结果转换为唯一的内存业务加载器。 */
@TargetApi(29)
final class MemoryPayloadLoader {
    private MemoryPayloadLoader() {}

    static ClassLoader create(Context context, ClassLoader parent) throws Exception {
        if (Build.VERSION.SDK_INT < 29) throw new IllegalStateException("M04");
        byte[][] dexes = Ld.decryptDexBytes(context);
        ByteBuffer[] buffers = new ByteBuffer[dexes.length];
        for (int index = 0; index < dexes.length; index++) {
            ByteBuffer buffer = ByteBuffer.allocateDirect(dexes[index].length);
            buffer.put(dexes[index]);
            buffer.flip();
            buffers[index] = buffer;
        }
        return new InMemoryDexClassLoader(buffers, nativeSearchPath(context), parent);
    }

    private static String nativeSearchPath(Context context) {
        ApplicationInfo info = context.getApplicationInfo();
        Set<String> paths = new LinkedHashSet<>();
        if (info.nativeLibraryDir != null && !info.nativeLibraryDir.isEmpty()) {
            paths.add(info.nativeLibraryDir);
        }
        if (info.sourceDir != null && !info.sourceDir.isEmpty()) {
            for (String abi : Build.SUPPORTED_ABIS) {
                if (abi != null && !abi.isEmpty()) {
                    paths.add(info.sourceDir + "!/lib/" + abi);
                }
            }
        }
        StringBuilder result = new StringBuilder();
        for (String path : paths) {
            if (result.length() > 0) result.append(File.pathSeparatorChar);
            result.append(path);
        }
        return result.toString();
    }
}

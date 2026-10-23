package dev.mocika.shield.loader;

import android.content.Context;
import android.content.pm.ApplicationInfo;
import android.content.pm.PackageManager;
import android.os.Bundle;

/** 只读取当前壳协议的元数据，不拥有缓存或环境策略。 */
final class RuntimeManifest {
    private RuntimeManifest() {}

    static Bundle read(Context context) {
        try {
            ApplicationInfo info = context.getPackageManager().getApplicationInfo(
                    context.getPackageName(), PackageManager.GET_META_DATA);
            if (info.metaData == null) throw new SecurityException("T01");
            return info.metaData;
        } catch (Exception error) {
            throw new SecurityException("T01", error);
        }
    }

    static String requiredText(Object value) {
        if (!(value instanceof String) || ((String) value).trim().isEmpty()) {
            throw new SecurityException("T02");
        }
        return (String) value;
    }
}

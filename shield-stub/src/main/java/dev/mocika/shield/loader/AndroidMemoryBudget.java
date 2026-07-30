package dev.mocika.shield.loader;

import android.app.ActivityManager;
import android.content.Context;
import android.os.Build;
import android.os.Process;

/** 只负责从 Android 系统采集内存预算快照。 */
final class AndroidMemoryBudget {
    private AndroidMemoryBudget() {}

    static MemoryBudgetSnapshot capture(Context context, long payloadDexBytes) {
        ActivityManager manager = (ActivityManager) context.getSystemService(
                Context.ACTIVITY_SERVICE);
        if (manager == null) {
            return new MemoryBudgetSnapshot(Build.VERSION.SDK_INT, 0, 0,
                    true, Process.is64Bit(), payloadDexBytes);
        }
        ActivityManager.MemoryInfo info = new ActivityManager.MemoryInfo();
        manager.getMemoryInfo(info);
        return new MemoryBudgetSnapshot(Build.VERSION.SDK_INT, info.totalMem, info.availMem,
                manager.isLowRamDevice(), Process.is64Bit(), payloadDexBytes);
    }
}

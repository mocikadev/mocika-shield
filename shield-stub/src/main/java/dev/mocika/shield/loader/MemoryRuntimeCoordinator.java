package dev.mocika.shield.loader;

import android.annotation.TargetApi;
import android.app.Application;
import android.content.Context;
import android.content.pm.ApplicationInfo;
import android.content.pm.PackageManager;
import android.os.Bundle;

import java.io.File;
import java.util.List;

/** 编排当前进程的一次加载尝试，不拥有具体解密或存储实现。 */
@TargetApi(28)
final class MemoryRuntimeCoordinator {
    private static final String CACHE_ROOT = "dev.mocika.shield.CACHE_ROOT_SHA256";
    private static final String PAYLOAD_DEX_BYTES = "dev.mocika.shield.PAYLOAD_DEX_BYTES";
    private static Attempt activeAttempt;

    private MemoryRuntimeCoordinator() {}

    static synchronized ClassLoader initialize(Context context, ClassLoader defaultLoader)
            throws Exception {
        if (activeAttempt != null) return activeAttempt.loader;
        if (!usesMemory(android.os.Build.VERSION.SDK_INT)) {
            List<File> dexFiles = Ld.extractDexFiles(context);
            DexInjector.inject(context, defaultLoader, dexFiles);
            return defaultLoader;
        }
        MemoryRuntimeProfiler profiler = MemoryRuntimeProfiler.start();
        PayloadDescriptor payload = payloadDescriptor(context);
        MemoryBudgetDecider.Decision budget = MemoryBudgetDecider.decide(
                AndroidMemoryBudget.capture(context, payload.dexBytes));
        RecoveryStateStore store = new RecoveryStateStore(
                context, Application.getProcessName());
        RecoveryStateStore.Record record = store.read();
        RecoveryStateMachine.Previous previous = record == null ? null
                : new RecoveryStateMachine.Previous(record.identity, record.state);
        RecoveryStateMachine.Plan plan = RecoveryStateMachine.begin(
                payload.identity, previous, budget.allowed);
        RecoveryStateMachine.Mode mode = RecoveryStateMachine.mode(plan);
        store.write(payload.identity, RecoveryStateMachine.pending(mode));
        if (profiler != null) profiler.stage("state_ready", 0, 0);

        ClassLoader loader;
        if (mode == RecoveryStateMachine.Mode.MEMORY) {
            loader = MemoryPayloadLoader.create(context, defaultLoader, profiler);
        } else {
            List<File> dexFiles = Ld.extractDexFiles(context);
            DexInjector.inject(context, defaultLoader, dexFiles);
            loader = defaultLoader;
            if (profiler != null) profiler.stage("file_fallback", dexFiles.size(), 0);
        }
        activeAttempt = new Attempt(payload.identity, plan, loader, store, profiler);
        if (profiler != null) profiler.stage("runtime_ready", 0, 0);
        return loader;
    }

    static boolean usesMemory(int sdkInt) {
        return sdkInt >= 31;
    }

    static synchronized void complete() throws Exception {
        Attempt attempt = activeAttempt;
        if (attempt == null) return;
        attempt.store.write(attempt.identity, RecoveryStateMachine.complete(attempt.plan));
        if (attempt.profiler != null) attempt.profiler.stage("application_ready", 0, 0);
    }

    private static PayloadDescriptor payloadDescriptor(Context context) throws Exception {
        ApplicationInfo info = context.getPackageManager().getApplicationInfo(
                context.getPackageName(), PackageManager.GET_META_DATA);
        Bundle metadata = info.metaData;
        String identity = metadata == null ? null : metadata.getString(CACHE_ROOT);
        if (identity == null || !identity.matches("[0-9a-f]{64}")) {
            throw new SecurityException("R14");
        }
        String dexBytesText = metadata == null ? null : metadata.getString(PAYLOAD_DEX_BYTES);
        long dexBytes;
        try {
            dexBytes = dexBytesText != null && dexBytesText.startsWith("bytes:")
                    ? Long.parseLong(dexBytesText.substring(6)) : 0;
        } catch (Exception error) {
            dexBytes = 0;
        }
        return new PayloadDescriptor(identity, dexBytes);
    }

    private static final class PayloadDescriptor {
        final String identity;
        final long dexBytes;

        PayloadDescriptor(String identity, long dexBytes) {
            this.identity = identity;
            this.dexBytes = dexBytes;
        }
    }

    private static final class Attempt {
        final String identity;
        final RecoveryStateMachine.Plan plan;
        final ClassLoader loader;
        final RecoveryStateStore store;
        final MemoryRuntimeProfiler profiler;

        Attempt(String identity, RecoveryStateMachine.Plan plan, ClassLoader loader,
                RecoveryStateStore store, MemoryRuntimeProfiler profiler) {
            this.identity = identity;
            this.plan = plan;
            this.loader = loader;
            this.store = store;
            this.profiler = profiler;
        }
    }
}

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
@TargetApi(29)
final class MemoryRuntimeCoordinator {
    private static final String CACHE_ROOT = "dev.mocika.shield.CACHE_ROOT_SHA256";
    private static Attempt activeAttempt;

    private MemoryRuntimeCoordinator() {}

    static synchronized ClassLoader initialize(Context context, ClassLoader defaultLoader)
            throws Exception {
        if (activeAttempt != null) return activeAttempt.loader;
        String identity = payloadIdentity(context);
        RecoveryStateStore store = new RecoveryStateStore(
                context, Application.getProcessName());
        RecoveryStateStore.Record record = store.read();
        RecoveryStateMachine.Previous previous = record == null ? null
                : new RecoveryStateMachine.Previous(record.identity, record.state);
        RecoveryStateMachine.Mode mode = RecoveryStateMachine.begin(identity, previous);
        store.write(identity, RecoveryStateMachine.pending(mode));

        ClassLoader loader;
        if (mode == RecoveryStateMachine.Mode.MEMORY) {
            loader = MemoryPayloadLoader.create(context, defaultLoader);
        } else {
            List<File> dexFiles = Ld.extractDexFiles(context);
            DexInjector.inject(context, defaultLoader, dexFiles);
            loader = defaultLoader;
        }
        activeAttempt = new Attempt(identity, mode, loader, store);
        return loader;
    }

    static synchronized void complete() throws Exception {
        Attempt attempt = activeAttempt;
        if (attempt == null) return;
        attempt.store.write(attempt.identity, RecoveryStateMachine.complete(attempt.mode));
    }

    private static String payloadIdentity(Context context) throws Exception {
        ApplicationInfo info = context.getPackageManager().getApplicationInfo(
                context.getPackageName(), PackageManager.GET_META_DATA);
        Bundle metadata = info.metaData;
        String identity = metadata == null ? null : metadata.getString(CACHE_ROOT);
        if (identity == null || !identity.matches("[0-9a-f]{64}")) {
            throw new SecurityException("R14");
        }
        return identity;
    }

    private static final class Attempt {
        final String identity;
        final RecoveryStateMachine.Mode mode;
        final ClassLoader loader;
        final RecoveryStateStore store;

        Attempt(String identity, RecoveryStateMachine.Mode mode, ClassLoader loader,
                RecoveryStateStore store) {
            this.identity = identity;
            this.mode = mode;
            this.loader = loader;
            this.store = store;
        }
    }
}

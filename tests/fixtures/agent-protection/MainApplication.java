package dev.mocika.shield.fixture;

/** 只用于静态完整加固验收，不采集信息，也不访问网络。 */
public final class MainApplication extends android.app.Application {
    @Override
    public void onCreate() {
        super.onCreate();
    }
}

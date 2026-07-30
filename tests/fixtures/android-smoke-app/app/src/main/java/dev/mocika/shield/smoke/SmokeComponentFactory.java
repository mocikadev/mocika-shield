package dev.mocika.shield.smoke;

import android.annotation.TargetApi;
import android.app.Activity;
import android.app.AppComponentFactory;
import android.app.Application;
import android.app.Service;
import android.content.BroadcastReceiver;
import android.content.ContentProvider;
import android.content.Intent;
import android.util.Log;

/** 验证候选壳完整委托原应用组件工厂。 */
@TargetApi(28)
public final class SmokeComponentFactory extends AppComponentFactory {
    private static void mark(String component) {
        Log.i("MocikaSmoke", "MOCIKA_SMOKE_FACTORY_" + component);
    }

    @Override
    public Application instantiateApplication(ClassLoader loader, String name)
            throws InstantiationException, IllegalAccessException, ClassNotFoundException {
        mark("APPLICATION");
        return super.instantiateApplication(loader, name);
    }

    @Override
    public Activity instantiateActivity(ClassLoader loader, String name, Intent intent)
            throws InstantiationException, IllegalAccessException, ClassNotFoundException {
        mark("ACTIVITY");
        return super.instantiateActivity(loader, name, intent);
    }

    @Override
    public Service instantiateService(ClassLoader loader, String name, Intent intent)
            throws InstantiationException, IllegalAccessException, ClassNotFoundException {
        mark("SERVICE");
        return super.instantiateService(loader, name, intent);
    }

    @Override
    public BroadcastReceiver instantiateReceiver(
            ClassLoader loader, String name, Intent intent)
            throws InstantiationException, IllegalAccessException, ClassNotFoundException {
        mark("RECEIVER");
        return super.instantiateReceiver(loader, name, intent);
    }

    @Override
    public ContentProvider instantiateProvider(ClassLoader loader, String name)
            throws InstantiationException, IllegalAccessException, ClassNotFoundException {
        mark("PROVIDER");
        return super.instantiateProvider(loader, name);
    }
}

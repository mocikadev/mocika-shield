package dev.mocika.shield.memorypayload;

import android.app.Activity;
import android.app.AppComponentFactory;
import android.app.Application;
import android.app.Service;
import android.content.BroadcastReceiver;
import android.content.ContentProvider;
import android.content.Intent;
import android.util.Log;

/** 模拟原应用已有的自定义组件工厂。 */
public final class PayloadAppComponentFactory extends AppComponentFactory {
    private static final String TAG = "MOCIKA_MEMORY_PROBE";

    @Override
    public Application instantiateApplication(ClassLoader loader, String className)
            throws InstantiationException, IllegalAccessException, ClassNotFoundException {
        Log.i(TAG, "ORIGINAL_FACTORY_APPLICATION");
        return super.instantiateApplication(loader, className);
    }

    @Override
    public Activity instantiateActivity(ClassLoader loader, String className, Intent intent)
            throws InstantiationException, IllegalAccessException, ClassNotFoundException {
        Log.i(TAG, "ORIGINAL_FACTORY_ACTIVITY");
        return super.instantiateActivity(loader, className, intent);
    }

    @Override
    public Service instantiateService(ClassLoader loader, String className, Intent intent)
            throws InstantiationException, IllegalAccessException, ClassNotFoundException {
        Log.i(TAG, "ORIGINAL_FACTORY_SERVICE");
        return super.instantiateService(loader, className, intent);
    }

    @Override
    public BroadcastReceiver instantiateReceiver(
            ClassLoader loader, String className, Intent intent)
            throws InstantiationException, IllegalAccessException, ClassNotFoundException {
        Log.i(TAG, "ORIGINAL_FACTORY_RECEIVER");
        return super.instantiateReceiver(loader, className, intent);
    }

    @Override
    public ContentProvider instantiateProvider(ClassLoader loader, String className)
            throws InstantiationException, IllegalAccessException, ClassNotFoundException {
        Log.i(TAG, "ORIGINAL_FACTORY_PROVIDER");
        return super.instantiateProvider(loader, className);
    }
}

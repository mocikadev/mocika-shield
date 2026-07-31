package dev.mocika.shield.smoke;

import android.app.Application;

import com.alibaba.android.arouter.core.LogisticsCenter;
import com.alibaba.android.arouter.facade.Postcard;
import com.alibaba.android.arouter.launcher.ARouter;

import java.lang.reflect.Method;

/** 初始化真实 ARouter 并观察路由表与目标业务方法。 */
public final class ARouterDexSeparationReporter {
    private ARouterDexSeparationReporter() {}

    public static String snapshot(Application application) {
        try {
            ARouter.openLog();
            ARouter.openDebug();
            ARouter.init(application);
            Postcard postcard = ARouter.getInstance().build("/research/target");
            LogisticsCenter.completion(postcard);
            Class<?> destination = postcard.getDestination();
            Method routeValue = destination.getDeclaredMethod("routeValue", int.class);
            return postcard.getPath() + ":" + routeValue.invoke(null, 10);
        } catch (Exception error) {
            return "missing:" + error.getClass().getName() + ":" + error.getMessage();
        }
    }
}

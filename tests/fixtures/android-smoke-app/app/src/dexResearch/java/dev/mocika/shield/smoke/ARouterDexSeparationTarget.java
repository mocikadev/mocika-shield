package dev.mocika.shield.smoke;

import android.app.Activity;

import com.alibaba.android.arouter.facade.annotation.Route;

/** 仅用于研究构建的真实 ARouter 目标。 */
@Route(path = "/research/target")
public final class ARouterDexSeparationTarget extends Activity {
    public static String routeValue(int input) {
        return "target-" + (input + 8);
    }
}

package dev.mocika.shield.smoke;

import android.app.Activity;
import android.os.Bundle;
import android.widget.TextView;
import android.util.Log;

import java.lang.reflect.Method;

public final class MainActivity extends Activity {
    @Override
    protected void onCreate(Bundle savedInstanceState) {
        super.onCreate(savedInstanceState);
        Log.i("MocikaSmoke", "MOCIKA_SMOKE_ACTIVITY_OK");
        verifySecondaryDex();
        TextView content = new TextView(this);
        content.setText("Mocika Shield 端到端测试");
        setContentView(content);
    }

    private static void verifySecondaryDex() {
        try {
            Class<?> markerClass = Class.forName("dev.mocika.shield.smoke.SecondaryMarker");
            Method verify = markerClass.getDeclaredMethod("verify");
            verify.invoke(null);
        } catch (Exception error) {
            throw new IllegalStateException("第二个 DEX 加载失败", error);
        }
    }
}

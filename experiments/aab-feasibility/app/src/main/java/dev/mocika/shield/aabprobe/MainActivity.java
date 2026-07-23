package dev.mocika.shield.aabprobe;

import android.app.Activity;
import android.os.Bundle;
import android.util.Log;
import android.widget.TextView;

public final class MainActivity extends Activity {
    private static final String LOG_TAG = "AabProbe";

    @Override
    protected void onCreate(Bundle state) {
        super.onCreate(state);
        TextView text = new TextView(this);
        String secondDexMessage = loadSecondDexMessage();
        Log.i(LOG_TAG, secondDexMessage);
        text.setText("AAB 尾部载荷验证应用已启动\n" + secondDexMessage);
        text.setTextSize(22);
        text.setPadding(48, 96, 48, 48);
        setContentView(text);
    }

    private String loadSecondDexMessage() {
        try {
            Class<?> messageClass = Class.forName("dev.mocika.shield.aabprobe.SecondDexMessage");
            return (String) messageClass.getMethod("text").invoke(null);
        } catch (ReflectiveOperationException error) {
            return "第二个 DEX 加载失败：" + error.getClass().getSimpleName();
        }
    }
}

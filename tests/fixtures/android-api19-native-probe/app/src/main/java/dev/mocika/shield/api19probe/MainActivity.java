package dev.mocika.shield.api19probe;

import android.app.Activity;
import android.os.Bundle;
import android.widget.TextView;

public final class MainActivity extends Activity {
    @Override
    protected void onCreate(Bundle savedInstanceState) {
        super.onCreate(savedInstanceState);
        TextView content = new TextView(this);
        content.setText(
                ProbeApplication.isNativeLoaded()
                        ? "Android 4.4 Native 加载成功"
                        : "Android 4.4 Native 加载失败");
        setContentView(content);
    }
}

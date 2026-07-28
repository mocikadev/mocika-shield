package dev.mocika.shield.loader;

import android.content.pm.ApplicationInfo;

import org.junit.Test;

import java.util.Arrays;
import java.util.Iterator;
import java.util.Set;

import static org.junit.Assert.assertEquals;
import static org.junit.Assert.assertFalse;
import static org.junit.Assert.assertTrue;

public class ARouterCompatTest {

    @Test
    public void 仅保留ARouter生成类并去重() {
        Set<String> routes = ARouterCompat.filterRouteClassNames(Arrays.asList(
                "  com.alibaba.android.arouter.routes.ARouter$$Root$$app  ",
                "com.example.NotARouterRoute",
                "",
                null,
                "com.alibaba.android.arouter.routes.ARouter$$Root$$app",
                "com.alibaba.android.arouter.routes.ARouter$$Providers$$app"));

        assertEquals(2, routes.size());
        Iterator<String> iterator = routes.iterator();
        assertEquals("com.alibaba.android.arouter.routes.ARouter$$Root$$app", iterator.next());
        assertEquals("com.alibaba.android.arouter.routes.ARouter$$Providers$$app", iterator.next());
    }

    @Test
    public void 空输入不产生路由类() {
        assertEquals(0, ARouterCompat.filterRouteClassNames(Arrays.<String>asList()).size());
    }

    @Test
    public void 可调试包保留提前注册路径() {
        assertTrue(ARouterCompat.shouldPreRegister(ApplicationInfo.FLAG_DEBUGGABLE));
        assertFalse(ARouterCompat.shouldPreRegister(0));
    }
}

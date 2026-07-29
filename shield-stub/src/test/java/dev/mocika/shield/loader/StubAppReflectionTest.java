package dev.mocika.shield.loader;

import org.junit.Test;

import static org.junit.Assert.assertEquals;
import static org.junit.Assert.fail;

public class StubAppReflectionTest {

    @Test
    public void 可以写入当前类的私有字段() throws Exception {
        Child target = new Child();

        StubApp.setField(Child.class, target, "childValue", "new-child");

        assertEquals("new-child", target.childValue);
    }

    @Test
    public void 可以向父类查找并写入私有字段() throws Exception {
        Child target = new Child();

        StubApp.setField(Child.class, target, "parentValue", "new-parent");

        assertEquals("new-parent", target.parentValue());
    }

    @Test
    public void 字段不存在时向调用方抛出异常() throws Exception {
        try {
            StubApp.setField(Child.class, new Child(), "missing", "value");
            fail("字段缺失后仍继续执行");
        } catch (NoSuchFieldException error) {
            assertEquals("missing", error.getMessage());
        }
    }

    private static class Parent {
        private String parentValue = "parent";

        String parentValue() {
            return parentValue;
        }
    }

    private static final class Child extends Parent {
        private String childValue = "child";
    }
}

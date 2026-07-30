package dev.mocika.shield.loader;

/** 在壳获得 Context 前保持加载器身份稳定，初始化后再转交业务类加载。 */
final class DeferredPayloadClassLoader extends ClassLoader {
    private volatile ClassLoader delegate;

    DeferredPayloadClassLoader(ClassLoader defaultLoader) {
        super(defaultLoader);
    }

    synchronized void initialize(ClassLoader candidate) {
        if (candidate == null) throw new IllegalArgumentException("M01");
        if (delegate != null) {
            if (delegate != candidate) throw new IllegalStateException("M02");
            return;
        }
        delegate = candidate;
    }

    boolean isInitialized() {
        return delegate != null;
    }

    @Override
    protected Class<?> findClass(String name) throws ClassNotFoundException {
        ClassLoader current = delegate;
        if (current == null) throw new ClassNotFoundException("M03:" + name);
        return current.loadClass(name);
    }
}

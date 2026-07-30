package dev.mocika.shield.memoryprobe;

/** 在壳获得 Context 前保持稳定身份，初始化后把业务类交给内存加载器。 */
final class DeferredPayloadClassLoader extends ClassLoader {
    private volatile ClassLoader delegate;

    DeferredPayloadClassLoader(ClassLoader defaultLoader) {
        super(defaultLoader);
    }

    synchronized void initialize(ClassLoader candidate) {
        if (candidate == null) {
            throw new IllegalArgumentException("MEMORY_PROBE_DELEGATE_NULL");
        }
        if (delegate != null) {
            if (delegate != candidate) {
                throw new IllegalStateException("MEMORY_PROBE_DELEGATE_ALREADY_SET");
            }
            return;
        }
        delegate = candidate;
    }

    @Override
    protected Class<?> findClass(String name) throws ClassNotFoundException {
        ClassLoader current = delegate;
        if (current == null) {
            throw new ClassNotFoundException("MEMORY_PROBE_PAYLOAD_NOT_READY:" + name);
        }
        return current.loadClass(name);
    }
}

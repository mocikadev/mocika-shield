package dev.mocika.shield.loader;

/** 负责在每次进程启动时执行环境安全检查，不参与 DEX 缓存与类加载。 */
final class RuntimeSecurity {

    private RuntimeSecurity() {
    }

    static void checkEnvironment() {
        enforceSafe(Ld.r());
    }

    static void enforceSafe(boolean unsafe) {
        if (unsafe) {
            throw new SecurityException("S01");
        }
    }
}

# Mocika Shield 运行时库 ProGuard 规则
#
# 整体策略：allowobfuscation 允许 R8 自由混淆类名和成员名。
# 构建脚本（build-stub.sh）会在 R8 产出 mapping.txt 后，重新编译 Rust，
# 将混淆后的类名/方法名通过环境变量注入编译期常量，保持 DEX 与 .so 一致。

# 默认 Native 名称锁定规则由 Gradle 精确转换，不能靠叠加 allowobfuscation 覆盖。
# JNI 回调类完整保留成员，名称允许混淆；最终绑定由同批映射注入 Native。
-keep,allowobfuscation class dev.mocika.shield.loader.Ld { *; }

# StubApp：类名允许混淆（shield-cli 从 metadata.json 读取混淆后的类名）。
# 框架入口方法（attachBaseContext/onCreate/getApplicationContext）覆盖 Application 父类，
# R8 在有 android.jar 约束时不会重命名覆盖方法；显式声明仅为防止被裁剪。
# 其余方法（injectDexElements、makeRealApp 等）由 R8 从三个入口追踪保留，名称可混淆。
-keep,allowobfuscation class dev.mocika.shield.loader.StubApp {
    <init>();
    protected void attachBaseContext(android.content.Context);
    public void onCreate();
    public android.content.Context getApplicationContext();
}

# AppComponentFactory 由系统按 Manifest 类名创建，类名写入候选资源元数据。
# 所有框架回调必须整体保留，避免 R8 无法感知的系统调用被裁剪。
-keep,allowobfuscation class dev.mocika.shield.loader.StubComponentFactory { *; }

# 桥内包含旧系统不存在的 AppComponentFactory 引用，禁止 R8 内联回 StubApp。
-keep,allowobfuscation class dev.mocika.shield.loader.MemoryRuntimeBridge { *; }

# ARouterCompat：由 StubApp.onCreate 调用链追踪保留，无需显式 keep。
# 类名和方法名均可被 R8 混淆。

# 将壳类打包到固定前缀 msk，与 app 自身混淆产物（通常 a/b/c）隔离，避免类名冲突
-repackageclasses 'msk'

# 不混淆异常类名（便于 crash 日志分析）
-keepnames class * extends java.lang.Exception

# 保留行号（便于 crash 定位），不保留 SourceFile（隐藏 .java 文件名）
-keepattributes LineNumberTable

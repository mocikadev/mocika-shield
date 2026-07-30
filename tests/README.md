# 端到端回归测试

`fixtures/android-smoke-app` 是项目自有的最小 Android 测试夹具，只负责提供包含自定义 `Application` 和启动 `Activity` 的稳定输入 APK，不承载产品示例或业务功能。

`scripts/build-smoke-multidex.sh` 会把独立编译的 `SecondaryMarker` 写入 `classes2.dex`。`scripts/run-protect-e2e.sh` 以这个真实双 DEX APK 为输入，负责无设备回归链路：编译显式设置 `extractNativeLibs=false` 且原本不含 Native 库的测试 APK，临时生成测试证书、签名原始 APK、执行加固、再次签名，并验证签名、MSHD 块、壳 Native 库、Manifest 保留和全部 `.so` 不压缩存储，以及 `check-apk` 结果。

```bash
make build-stub
bash tests/scripts/run-protect-e2e.sh
```

连接 Android 设备后，可以额外验证未加固双 DEX 基线，以及使用同一证书覆盖安装加固包后的首次解密和缓存命中二次启动：

```bash
RUN_DEVICE_TEST=1 bash tests/scripts/run-protect-e2e.sh
```

普通非 Root 设备可额外执行严格环境策略回归，确认高置信 Root 检测不会误报：

```bash
ENVIRONMENT_POLICY=strict RUN_DEVICE_TEST=1 bash tests/scripts/run-protect-e2e.sh
```

已通过 `adb root` 获得 Root ADB 的 LineageOS 等设备，应要求严格策略拒绝加固包启动：

```bash
ENVIRONMENT_POLICY=strict EXPECT_PROTECTED_REJECTION=1 RUN_DEVICE_TEST=1 \
  bash tests/scripts/run-protect-e2e.sh
```

存在多个在线设备时通过 `ANDROID_SERIAL` 指定目标设备。测试证书只在系统临时目录中生成，脚本退出时自动清理，不向仓库提交任何私钥。日常 CI 不启动模拟器；真实运行时回归由维护者在真机或指定模拟器上按需执行。

## API 29+ 内存 DEX 加载探针

`fixtures/android-memory-loader-probe` 是隔离研究夹具，不进入正式 Stub 或资源包。它把 Application、Activity、Service、Provider 和跨 DEX 依赖编译到两个独立 DEX 资源中，启动壳只负责创建 `InMemoryDexClassLoader` 并替换框架 `LoadedApk` 持有的应用 ClassLoader。

连接 API 29 及以上设备后执行：

```bash
bash tests/scripts/run-memory-loader-probe.sh
```

探针必须同时确认主进程与远程 Service 进程分别完成加载器替换，Application、Provider、Activity、Service 与第二 DEX 类均正常创建，APK 内 Native 库可以通过业务类加载，并且 GC 后仍可首次访问延迟类；应用私有目录不得生成完整 DEX 文件。该结果只证明最小框架链路可行，不代表 ARouter、系统共享库、覆盖安装或生产回退已经通过。

## Android 4.4 Native 加载探针

`fixtures/android-api19-native-probe` 只负责验证 NDK r25c、Rust 1.77.2、API 19 构建的生产 `libmocikashield.so` 能够被 Android 4.4 的动态链接器加载，并成功执行 `JNI_OnLoad` 动态注册。它不包含 DEX 解密、Dalvik 注入或业务兼容逻辑。

先启动一个 API 19、`armeabi-v7a` 的模拟器或设备，再执行：

```bash
bash tests/scripts/run-api19-native-probe.sh
```

脚本会重复执行 Native ELF 审计、构建探针 APK、安装并启动，然后检查 `MOCIKA_API19_NATIVE_OK` 日志。存在多个在线设备时通过 `ANDROID_SERIAL` 指定目标设备。

完整 Dalvik 加固回归使用双 DEX 测试 APK，验证 Native 解密、Element 前插、自定义 Application、第二 DEX 类、首次安装、清除数据和同签名覆盖安装：

```bash
ANDROID_SERIAL=emulator-5554 bash tests/scripts/run-api19-protect-e2e.sh
```

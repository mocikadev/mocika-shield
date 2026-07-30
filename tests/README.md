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

`fixtures/android-memory-loader-probe` 是隔离研究夹具，不进入正式 Stub 或资源包。它把 Application、Activity、Service、Provider 和跨 DEX 依赖编译到两个独立 DEX 资源中，并分别构建反射写入 `LoadedApk.mClassLoader` 与公开 `AppComponentFactory.instantiateClassLoader()` 两种入口进行对照。

连接 API 29 及以上设备后执行：

```bash
bash tests/scripts/run-memory-loader-probe.sh
```

脚本会依次安装两个变体。两者都必须确认主进程与远程 Service 进程分别创建业务加载器，Application、Provider、Activity、Service、Receiver 与第二 DEX 类均正常创建，APK 内 Native 库可以通过业务类加载，并且 GC 后仍可首次访问延迟类；应用私有目录不得生成完整 DEX 文件。

工厂变体先由 `AppComponentFactory` 返回未初始化的稳定代理，确认业务类在获得 Context 前不能加载；随后在壳 Application 的 `attachBaseContext()` 中创建真实内存加载器并挂接代理。测试同时验证重复初始化不改变加载器、两个线程并发首次加载业务类，以及载荷中的原应用工厂收到五类组件实例化回调。该结果只证明最小框架链路可行，不代表正式 Stub Native、DEXB v5 签名绑定解密、原工厂元数据、ARouter、系统共享库、覆盖安装或生产回退已经通过。

当前已通过 API 29 ARM64 4 KB、API 35 ARM64 16 KB、API 36 ARM64 4 KB 模拟器和 API 35 ARM64 真机。探针 Native 库显式生成 SysV/GNU 双哈希表并按 16 KB 最大页大小链接，防止测试资产自身在 16 KB 系统中先于内存 DEX 验证失败。

完成上述框架探针后，可在 API 29 以上设备验证正式 Stub Native 与 DEXB v5 签名绑定解密：

```bash
BUNDLETOOL_JAR=/path/to/bundletool.jar \
  bash tests/scripts/run-memory-loader-dexb-probe.sh
```

该脚本要求已执行 `make build-stub`，并通过 `BUNDLETOOL_JAR` 指定本机 bundletool。它使用项目核心加固流程和临时证书生成真实 DEXB v5 双 DEX 载荷，把正式 `libmocikashield.so` 装入工厂变体，并验证同签名时主进程、远程进程和全部业务生命周期正常且私有目录无 DEX；随后验证同签名外部 Instrumentation、设备 split 集安装和异签名失败关闭。动态特性代码会由 bundletool 合入安装时 master split，脚本会确认代理能够访问该类，同时确认该类仍以明文 DEX 存在。因此这里只证明 split 运行兼容，不能宣称代码 split 已获得保护。所有证书和中间 APK 都只存放在系统临时目录，退出时自动清理。

当前正式 Native/DEXB 链路已通过 API 35 ARM64 真机；其他 API 节点仍沿用框架明文载荷矩阵，不能据此宣称正式解密链路已覆盖全部系统版本。

正式候选资源的系统边界使用独立端到端脚本验证。产物准备脚本会用同一临时证书生成标准资源和内存候选资源的真实双 DEX 加固包；设备脚本检查原组件工厂五类回调和主/远程进程，并按设备系统版本自动断言：API 28～30 使用认证文件缓存且不生成内存状态，API 31 以上生成分进程认证状态且不落盘明文 DEX。

```bash
ANDROID_SERIAL=emulator-5554 bash tests/scripts/run-memory-runtime-e2e.sh
```

API 31 以上还会依次验证清除数据、标准与候选资源双向覆盖迁移、真实 Application 异常后的认证文件回退、文件路径连续失败、主/远程进程状态隔离、状态 MAC 损坏、包装密文损坏和 Android Keystore 条目删除。脚本兼容系统自动重试和等待显式重启两种行为。

连续验证多个设备时，首次执行完成构建后可以设置 `SKIP_BUILD=1` 复用仓库构建产物；每次仍会重新生成临时证书和两种加固包，不在仓库留下密钥或 APK。`prepare-memory-runtime-e2e-apks.sh` 只负责同签名测试产物，`run-memory-runtime-e2e.sh` 只负责编排设备状态和断言。

使用真实、已签名的 AndroidX/ARouter APK 验证原组件工厂恢复和三种安装状态：

```bash
bash tests/scripts/run-memory-loader-arouter-probe.sh \
  /path/to/arouter-signed.apk \
  /path/to/sign-apk.sh
```

签名脚本接口固定为 `<输入 APK> <输出 APK>`。脚本会先用该证书重新签名原始样本，确保未加固基线、认证文件路径和内存加载原型能够同签名覆盖；随后保留样本资源、Manifest 组件、Native 库和 ARouter 路由清单，验证 AndroidX 原组件工厂、正式 DEXB v5 解密及 `/home/main` 路由。当前样本在 API 35 ARM64 真机的文件到内存、内存到文件、再次切回内存、清除数据和全新安装场景均通过。

性能阶段使用不自动路由、不预加载首页类且不开启观测日志的独立 APK，分别采集三种形态的五次冷启动、即时 TOTAL PSS 和统一内存整理后的稳定 PSS 中位数。诊断 APK 单独输出读取、解密、直接缓冲区复制、加载器创建及启动后 GC 快照，并用弱引用确认临时载荷对象是否回收。结果只代表当前设备和样本，不作为跨设备性能承诺。脚本是隔离原型，不生成正式发布资源。

脚本还会构建两个故障注入变体：第一个使内存路径在业务 Application 启动前崩溃，验证下一独立进程进入认证文件回退且后续保持粘性；第二个继续使文件路径失败，验证遗留 `file_pending` 状态会在再次启动时失败关闭。脚本还会同时启动主进程与远程进程，确认两者使用独立认证状态且不会互相误判；随后分别破坏状态记录、删除包装密钥材料和损坏包装密文，确认均在业务类定义前失败关闭。认证记录绑定加密载荷身份和进程身份，HMAC 软件密钥由 Android Keystore 分进程包装。该状态机仍只属于隔离探针，不会写入正式 Stub 或资源包。

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

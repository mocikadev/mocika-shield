# Android 4.4 工控兼容设计

本文记录 Android 4.4（API 19～20）工控设备兼容方案、构建边界和验证要求。该能力仍处于可行性验证阶段，不代表当前稳定版本已经支持 Android 4.4。

## 目标与非目标

目标：

- 提供“标准模式”和“工控兼容模式”，标准模式不因旧系统适配而降低工具链版本。
- 工控兼容模式生成一个 APK，同时运行在 Android 4.4、5.0、6.0 及经过验证的更高版本。
- 保持 DEXB v5、签名校验、加密和解密协议一致，只隔离 Native 构建与 DEX 加载差异。
- 首期以 Issue #15 的 Android 4.4.2、`armeabi-v7a` 真实工控板完成端到端验证。

非目标：

- 首期不支持已经从现代 NDK 删除的 `armeabi`、`mips` 和 `mips64`。
- 不在用户执行加固时调用 NDK 或现场编译 Stub。
- 不复制 GUI、CLI、核心加固流程或维护第二个长期仓库。
- 不通过工控兼容模式降低原应用自己的 `minSdkVersion`。

## 工具链与 ABI

项目以四种主流 ABI 为基准：

| ABI | 标准模式 | 工控兼容模式 | 最低系统 |
|-----|----------|--------------|----------|
| `armeabi-v7a` | NDK r29 | NDK r25c | 兼容模式 API 19 |
| `x86` | NDK r29 | NDK r25c | 兼容模式 API 19 |
| `arm64-v8a` | NDK r29 | NDK r29 | API 21 |
| `x86_64` | NDK r29 | NDK r29 | API 21 |

固定版本：

```text
标准 NDK：29.0.14206865（r29）
兼容 NDK：25.2.9519653（r25c）
兼容 Rust：1.77.2
```

兼容构建必须使用 Rust 1.77.2。当前 Rust 1.97 的 Android unwind 会引用 API 21 才提供的 `dl_iterate_phdr`，即使传入 `--platform 19` 也只能完成链接，产物无法在 Android 4.4 被动态链接器加载。兼容构建使用 `shield-stub/compat/api19-rust` 下的独立 Cargo 清单与版本 3 锁文件，但通过 `lib.path` 和 `build` 引用同一份生产 Rust 源码；标准构建继续使用根锁文件，两套工具链不会互相改写依赖。更新 Native 依赖后必须重新执行 API 19 加载探针。

Android 4.4 官方没有 64 位 ABI。兼容资源中的 64 位 Stub 继续使用 r29/API 21，32 位 Stub 才使用 r25c/API 19。

NDK r24 起不再支持非 NEON 的 `armeabi-v7a` 设备，因此 r25c 产物要求 CPU 支持 NEON。若真实设备不支持 NEON，再单独评估 r23c；在出现真实需求前不引入第三套正式工具链。

## 资源与运行时边界

构建阶段预生成两套资源，用户加固时只做选择和注入：

```text
resources-standard.zip
└── 四种 ABI 均使用 r29/API 21

resources-legacy-api19.zip
├── armeabi-v7a：r25c/API 19
├── x86：r25c/API 19
├── arm64-v8a：r29/API 21
└── x86_64：r29/API 21
```

工控兼容 Stub 按系统版本选择 DEX 加载路径：

```text
API 19～20：Dalvik 兼容加载路径
API 21～23：现有 Element 工厂 ART 路径
API 24 以上：现有 addDexPath ART 路径
```

Dalvik 与 ART 可以拥有独立的加载实现，但以下逻辑必须共享：

- MSHD 和 DEXB v5 解析；
- Native 解密与签名校验；
- DEX 文件落地和缓存生命周期；
- 真实 Application 恢复；
- ARouter 等框架兼容处理；
- 错误语义和安全日志规范。

## 加固模式选择

GUI 已提供按任务选择的“标准模式”和“Android 4.4 工控兼容”模式，并在后端固定映射内置资源。当前不会根据 `minSdkVersion` 自动切换，仍由用户确认目标系统；CLI 暂不公开兼容模式参数。后续可读取原 APK 的 `minSdkVersion` 与 `lib/<ABI>/` 自动给出建议，但不得静默切换模式。

| 原 APK | 默认建议 |
|--------|----------|
| `minSdk >= 21` | 标准模式 |
| `minSdk <= 20` | 工控兼容模式 |
| 无法识别 | 标准模式并要求确认目标系统 |

原 APK 声明支持 API 19，但用户选择标准模式时，必须明确提示加固产物最低要求会提高到 API 21。

如果原 APK 包含 Native 库，只注入原 APK 已有 ABI 对应的 Stub，禁止通过 Stub 引入新的 ABI。纯 Java/Kotlin APK 才可以注入全部四种 ABI，避免 Android 选择了业务库并不完整的 ABI 后启动崩溃。

## 验证记录

2026-07-28 完成 Native 构建与最小加载验证：

- 本机并行安装 NDK r25c `25.2.9519653`，未替换标准构建的 r29。
- Rust 1.77.2 下，当前 Stub 及 `jni`、Zstd、ChaCha20-Poly1305、HKDF、SHA-256 依赖成功以 `armeabi-v7a/API 19` 完成 release 构建。
- ELF 为 32 位 ARM EABI5、ARMv7、Thumb-2、VFPv3 和 NEON。
- `.note.android.ident` 标记 API 19、r25c 和构建号 9519653。
- 动态依赖仅为 `libc.so` 与 `libdl.so`，未引用已知的 API 21 符号 `dl_iterate_phdr`。
- 在 Android 4.4.2、API 19、`armeabi-v7a` 模拟器中完成安装和冷启动，生产 `libmocikashield.so` 成功进入 `JNI_OnLoad` 并完成 JNI 动态注册。
- 对照验证确认 Rust 1.97 产物会因找不到 `dl_iterate_phdr` 在 `System.loadLibrary` 阶段失败，因此兼容工具链不能跟随标准构建升级。
- 已完成真实加固 APK 的 Dalvik 端到端回归：单 DEX和双 DEX均能解密注入，自定义 Application、Activity 与第二 DEX 类均成功加载。
- 首次安装、清除数据后启动、同签名覆盖安装均已通过。
- 同一个双 DEX 兼容加固 APK 已在 API 19 `armeabi-v7a`、API 21 `arm64-v8a` 和 API 23 `arm64-v8a` 模拟器通过，分别命中 Dalvik 与 ART Element 工厂路径，证明兼容资源不要求用户维护多份业务 APK。
- 加载段当前按 4 KB 对齐；该产物只用于 API 19 实验，不替代现代 16 KB 对齐产物。

可重复执行：

```bash
./scripts/verify-android-api19-native.sh
ANDROID_SERIAL=emulator-5554 ./tests/scripts/run-api19-native-probe.sh
ANDROID_SERIAL=emulator-5554 ./tests/scripts/run-api19-protect-e2e.sh
```

产物和审计报告位于 `shield-stub/build/experiments/api19/`，该目录属于构建输出，不进入版本库，也不会覆盖正式 `build/jniLibs` 与 `resources.zip`。

## 分阶段实施

1. **Native 可行性**：已完成 r25c/API 19 编译、ELF 和动态符号审计。
2. **最小加载验证**：已在 API 19 模拟器确认 Native 库能够由系统加载并进入 `JNI_OnLoad`。
3. **Dalvik 加载实现**：已实现 API 19～20 的 Element 前插，并在 API 19 验证双 DEX 与真实 Application 恢复。
4. **模拟器回归**：API 19、21、23 已使用同一加固 APK 通过首次安装、清除数据、同签名覆盖安装。
5. **真实工控板回归**：Android 6.0 工控板已验证；仍需使用同一次加固产物在用户 Android 4.4.2 工控板验证主要业务及硬件交互。
6. **产品接入**：GUI 双资源选择和 ABI 预检已完成；最低版本自动建议与公开 CLI 模式暂未实现，稳定支持前不作为必需项。

## 完成条件

- 同一个工控兼容加固 APK 在 Android 4.4.2 与 Android 6.0 真实工控板均正常运行。
- 单 DEX、多 DEX、Native 库、真实 Application、首次安装、清除数据、覆盖安装全部通过。
- 不回归 API 21 以上现有 ART、ARouter、签名校验和 16 KB 对齐能力。
- 用户无需为 Android 4.4 与 Android 6.0 维护两份业务 APK。

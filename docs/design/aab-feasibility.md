# AAB 加固可行性验证

本文档记录 Android App Bundle 加固的实验边界、验证方法和结论。当前工作属于技术探索，不表示正式版本已经支持 AAB。

## 目标

优先回答以下问题：

1. AAB `base/dex/classes.dex` 的 `file_size` 之外追加载荷后，bundletool 生成的拆分 APK 和通用 APK 是否保留该载荷。
2. Google Play 重新生成和签名 APK 后，载荷是否仍然保留。
3. 运行时签名绑定应如何区分上传证书、Play 应用签名证书和本地测试证书。
4. 单 base 模块验证通过后，现有 DEXB v5 是否能扩展到安装时模块和按需模块。

## 当前边界

- 仅验证单 base 模块。
- 首轮使用固定明文标记验证字节保真，第二轮使用正式 DEXB v5 packer 和现有 Android 壳验证运行时链路。
- `shield-core` 只增加 `aab-experiment` feature 下的隐藏实验入口；GUI、CLI 和正式 APK 加固接口不变。
- 不承诺动态功能模块、Asset Pack、Instant App 或第三方应用商店兼容性。
- 实验生成的 AAB、APKS、APK 和测试密钥不提交仓库。

## 关键差异

APK 是设备安装格式；AAB 是发布格式。Google Play 或 bundletool 会从 AAB 生成 base APK、配置 APK和功能模块 APK。AAB 本身使用上传密钥签名，设备上的 APK 通常由 Play 应用签名密钥签名。

因此，当前“从输入 APK 提取证书指纹并绑定 DEXB 密钥”的流程不能直接复用于 AAB。正式方案至少需要允许配置 Play 应用签名证书指纹，并为本地测试提供独立允许指纹。

## 本地实验

实验项目位于 `experiments/aab-feasibility/`。它负责生成最小 AAB，并执行字节级保真验证，不负责实现正式加固。

真实运行链路实验中，先把 `resources.zip` 的 JNI 库准备为 shell 模块的 Gradle 输入，再构建 AAB：

```bash
python3 experiments/aab-feasibility/prepare_runtime_jni.py \
  --resources shield-stub/build/outputs/resources/resources.zip \
  --output experiments/aab-feasibility/artifacts/runtime-jniLibs

./shield-stub/gradlew -p experiments/aab-feasibility :shell:bundleRelease
```

`assemble_dexb_aab.py` 只负责提取原始 DEX、校验 shell AAB 已包含全部运行库并替换壳 DEX，不再向已生成的 AAB 事后注入 Native 库。双 DEX 探针由 `prepare_multidex_aab.py` 生成，构建产物统一位于忽略提交的 `artifacts/`。

准备可执行的 `bundletool-all` jar 后运行：

```bash
./shield-stub/gradlew -p experiments/aab-feasibility :app:bundleRelease

python3 experiments/aab-feasibility/verify_tail_preservation.py \
  --aab experiments/aab-feasibility/app/build/outputs/bundle/release/app-release.aab \
  --bundletool /path/to/bundletool-all.jar \
  --output experiments/aab-feasibility/artifacts
```

脚本会生成默认拆分 APK 集和通用 APK 集，并检查每个 DEX：

- DEX 头部 `file_size` 保持原值。
- 固定实验标记位于物理文件末尾。
- 标记经过 bundletool 转换后仍存在。

## 判定规则

| 结果 | 结论 | 后续动作 |
|------|------|----------|
| 本地拆分与通用 APK 均保留 | 可以继续验证 DEXB v5 和运行时加载 | 进入签名绑定与安装实验 |
| 本地 bundletool 丢失或拒绝载荷 | 现有尾部容器不适用于 AAB | 评估新载荷容器，停止接入生产代码 |
| 本地保留但 Play 丢失或拒绝 | 只能用于非 Play AAB 工具链，产品价值有限 | 不宣称 Google Play AAB 支持 |
| Play 内部测试保留且运行正常 | 基础路径可行 | 建立正式 `feat/aab-support` 分支 |

## 后续测试矩阵

- 单 DEX与多 DEX
- bundletool 默认拆分与通用 APK
- arm64-v8a、armeabi-v7a、x86、x86_64
- 本地测试证书与 Play 应用签名证书
- 首次安装、覆盖更新、清除数据后启动
- 安装时功能模块、条件模块与按需模块
- Play 内部测试轨道生成的设备 APK

## 实验记录

### 2026-07-23：本地 bundletool 尾部保真实验

环境：

- macOS arm64
- AGP 8.13.2
- Gradle 8.14.3
- bundletool 1.18.3
- Gradle Launcher Java 17；Android Studio Daemon Java 21
- compileSdk / targetSdk 35，minSdk 23

输入 DEX 的头部 `file_size` 和物理长度均为 1352 字节。在 AAB 的 `base/dex/classes.dex` 末尾追加 24 字节固定标记后，物理长度为 1376 字节，头部 `file_size` 保持 1352。

结果：

| 输出 | 最终 DEX | 物理长度 | 标记位置 | 结果 |
|------|----------|----------|----------|------|
| 默认拆分 APK 集 | `splits/base-master.apk/classes.dex` | 1376 | 1352 至文件末尾 | 保留 |
| 默认拆分 APK 集的另一设备变体 | `splits/base-master_2.apk/classes.dex` | 1376 | 1352 至文件末尾 | 保留 |
| 通用 APK 集 | `universal.apk/classes.dex` | 1376 | 1352 至文件末尾 | 保留 |
| 加固后重新进行 JAR 签名的 AAB → 通用 APK 集 | `universal.apk/classes.dex` | 1376 | 1352 至文件末尾 | 保留 |
| Android 15 arm64 设备实际安装的 base APK | `base.apk/classes.dex` | 1376 | 1352 至文件末尾 | 保留 |

设备验证使用 Android 15、arm64-v8a 真机。`bundletool install-apks` 成功选择并安装拆分 APK，测试 Activity 冷启动成功且进程正常存活。从设备 `/data/app` 拉回的实际 base APK 为 12944 字节，其中 `classes.dex` 的头部 `file_size` 仍为 1352，物理长度为 1376，固定标记完整位于物理末尾。

明确结论：bundletool 1.18.3 不会清理 DEX `file_size` 之外的尾部载荷，且加固后重新使用 `jarsigner` 签名的 AAB 可以继续生成 APK 集；bundletool 的设备选择、安装和 Android 包管理器安装过程也不会清理该载荷。现有 DEXB v5 容器通过本地第一阶段与真机安装门禁，可以继续进行真实 DEXB 载荷和 Play 内部测试。

第一轮结论不能推断 Google Play 服务端一定保留载荷；真实 DEXB v5 与 Play 门禁分别在后续实验中验证。

### 2026-07-23：真实 DEXB v5 解密加载实验

实验使用正式 `shield-core` packer 将原始 `classes.dex` 打包为 DEXB v5，签名绑定目标为 bundletool 最终 APK 使用的 Android Debug 证书。组装阶段将当前 `resources.zip` 中的混淆壳 DEX 写入单 base 模块 AAB，并在壳 DEX 的头部 `file_size` 之外追加 `MSHD + payload_len + DEXB`。

关键数据：

| 项目 | 结果 |
|------|------|
| 原始业务 DEX | 1352 字节 |
| Zstd 压缩后业务 DEX | 770 字节 |
| DEXB v5 | 928 字节 |
| 壳 DEX 头部 `file_size` | 13128 字节 |
| 加入 MSHD 和 DEXB 后物理长度 | 14064 字节 |
| Native ABI | arm64-v8a、armeabi-v7a、x86、x86_64 |

真机结果：

- bundletool 成功生成并安装设备 APK 集。
- `libmocikashield.so` 从 base APK 的 arm64-v8a 路径加载成功。
- 壳 DEX 经 `dexdump` 确认不包含 `dev.mocika.shield.aabprobe.MainActivity`。
- 原始 Activity 冷启动成功，界面树显示“AAB 尾部载荷验证应用已启动”。
- 强制停止后再次启动成功，耗时约 170 毫秒。
- 清除应用数据后重新解密并启动成功，耗时约 207 毫秒。
- 使用另一张临时证书签署最终 APK 后，运行时按预期因 ChaCha20-Poly1305 密钥不匹配而拒绝启动；恢复绑定证书后再次启动成功。
- 正确签名路径没有解密失败、类加载失败或崩溃日志。

明确结论：单 base 模块的 AAB 可以复用现有 DEXB v5 packer、签名绑定、混淆壳 DEX、Native 解密和 PathClassLoader 注入链路。当前本地与真机门禁全部通过。

### 2026-07-23：ABI 分包、双 DEX 与覆盖更新实验

将四 ABI Native 库改为 shell 模块的 `jniLibs` 输入后，bundletool 生成了四个独立 ABI 配置 APK。arm64-v8a 真机实际只安装 `base.apk` 和 `split_config.arm64_v8a.apk`，运行日志确认 `libmocikashield.so` 从 arm64 配置 APK 加载，未再把其他三种 ABI 下发到设备。说明 AAB 正式方案应在模块构建阶段准备 Native 库，不能在 AAB 生成后注入。

双 DEX 实验将业务 Activity 放在 `classes.dex`，将反射调用的 `SecondDexMessage` 单独编译为 `classes2.dex`。正式 DEXB v5 packer 识别并加密了 2 个 DEX，真机启动日志输出“第二个 DEX 已加载”，证明当前壳的多 DEX 解密与类加载链路可用。

覆盖更新实验先安装 shell `versionCode=2`，再使用相同签名直接安装 `versionCode=3`，未卸载也未清除数据。更新后系统报告版本号为 3，应用重新启动并成功加载双 DEX 与 ABI 配置 APK 中的 Native 库，没有解密失败或崩溃。说明当前按版本隔离的解密缓存可通过基础覆盖更新门禁。

本地尚未验证的关键门禁只剩 Google Play 内部测试轨道及 Play 应用签名证书绑定。该结果也不能推断动态功能模块、Asset Pack 或按需交付已经兼容。

每次后续实验仍应记录输入摘要、输出摘要和明确结论，不提交测试密钥或用户应用。

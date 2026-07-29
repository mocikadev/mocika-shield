# Android Native 库打包与加载兼容设计

本文定义 Mocika Shield 加固过程中对 `android:extractNativeLibs`、APK 内 `.so` 压缩方式、ZIP 对齐和 ELF 页大小兼容性的处理规则。该设计用于修复原 APK 显式设置 `extractNativeLibs=false` 且原本不包含 Native 库时，加固产物无法安装的问题。

当前状态：核心修复、自动结构回归及候选版本设备矩阵均已完成。Android 16/API 36 已完成有/无原始 Native 库两类真机安装验证，API 23 与 API 35 16 KB 环境也已完成安装启动回归。

## 问题与复现证据

已使用项目 Smoke APK 在 Android 16/API 36 真机复现：

1. 原 APK 显式设置 `android:extractNativeLibs="false"`。
2. 原 APK 不包含 `lib/**/*.so`。
3. 加固后首次注入四个 ABI 的 `libmocikashield.so`。
4. 使用与原 APK 相同的证书签名。
5. 加固产物通过现有 16 KB ZIP 对齐检查，但四个壳库均为 ZIP 压缩条目。
6. 安装失败：

```text
INSTALL_FAILED_INVALID_APK: Failed to extract native libraries, res=-2
```

对照样本中，原 APK 已包含未压缩 Native 库时，apktool 会保留相应的不压缩规则，新注入壳库也可能保持未压缩，Android 16 可以正常安装启动。因此该缺陷只依赖现有样本是否带 Native 库，容易被常规回归遗漏。

## 根因

`extractNativeLibs=false` 表示系统直接从 APK 映射 Native 库，不在安装时将其解压到应用目录。此模式要求 APK 内相关 `.so` 至少同时满足：

- ZIP 条目使用 `Stored`，不得压缩。
- 条目数据偏移满足系统要求的页边界对齐。
- ELF `LOAD` 段满足目标设备页大小要求。

当前内置对齐器会把 `lib/**/*.so` 的数据位置调整到 16 KB 边界，但会保留 apktool 重打包后的压缩方式。因此“压缩的 `.so` 已对齐”仍然不能满足 `extractNativeLibs=false` 的直接加载契约。

## 目标

- 保留原 APK 的 `extractNativeLibs` 语义，不无条件改成 `true`。
- 原 APK 显式为 `false` 时，确保全部 Native 库不压缩并按 16 KB 对齐。
- 将压缩方式、ZIP 对齐和 ELF 页大小兼容作为三项独立条件验证。
- 原 APK 为 `true` 或未设置时，避免无意义地把全部业务 `.so` 改为不压缩并放大 APK。
- 同时覆盖“原 APK 已有 Native 库”和“加固后才首次出现 Native 库”两类场景。

## 非目标

- 不改变原应用选择的 Native 库提取策略。
- 不把 `extractNativeLibs=true` 作为正式修复。
- 不在 GUI 增加用户可选开关；这是 APK 打包一致性规则，不是保护偏好。
- 不以 ZIP 对齐通过代替 ELF `LOAD` 段检查。
- 不在本任务调整 Stub ABI 选择和 Android 4.4 双 NDK 策略。

## 策略模型

加固时从 apktool 解码后的 Manifest 读取原始 `<application>` 属性，归一化为：

| 状态 | 含义 | 打包规则 |
|------|------|----------|
| `Disabled` | 显式 `extractNativeLibs=false` | 所有 `lib/**/*.so` 强制 `Stored`，并按 16 KB 对齐 |
| `Enabled` | 显式 `extractNativeLibs=true` | 保留重打包后的压缩方式，不修改 Manifest |
| `Unspecified` | 未设置 | 保留平台和原构建策略，不主动写入 Manifest |

不得把“未设置”直接等同于 `true` 写回 Manifest。枚举只属于当前加固任务，不进入 GUI 请求、`config.toml` 或资源协议。

## 模块边界与数据流

```text
apktool 解包
  → Manifest：读取 NativeLibPackagingPolicy
  → 注入 Runtime Native 库
  → apktool 重打包
  → ZIP 重写：按策略决定 .so 压缩方式并执行对齐
  → 产物验证：压缩方式 + ZIP 对齐 + ELF 能力
  → 输出未签名加固 APK
```

| 单元 | 负责 | 不负责 |
|------|------|--------|
| `protect::manifest` | 解析原始 `extractNativeLibs` 三态策略 | 修改用户原始策略、重写 ZIP |
| `protect_api` | 在单次加固流程中传递策略并编排重打包、对齐与验证 | 解析 ZIP 细节 |
| `zipalign` | 按显式策略重写 `.so` 的压缩方式和数据对齐，报告违规条目 | 决定 Manifest 语义、选择 Stub ABI |
| Native ELF 审计 | 检查 Stub 的 ELF 架构、依赖和 `LOAD` 段对齐 | 修改 APK Manifest 或 ZIP |
| GUI / CLI | 展示核心返回的通用错误 | 提供 `extractNativeLibs` 开关或自行修补 Manifest |

签名页只对既有 APK 做通用对齐，不掌握原始解码 Manifest，不能借签名流程补救错误的加固产物；一致性必须在核心加固输出阶段完成。

## ZIP 重写规则

现有 `align_apk()` 默认保留每个 ZIP 条目的压缩方式。加固流程通过内部策略参数选择 Native 库处理方式，不改变签名等其他调用方的默认行为：

```text
Preserve
  → 保留原条目压缩方式

StoreNativeLibraries
  → lib/**/*.so 使用 Stored
  → lib/**/*.so 使用 16 KB 对齐
  → 其他条目保留原压缩方式并使用现有对齐规则
```

不建议通过修改 apktool YAML 作为唯一修复，因为不同 apktool 版本、输入 APK 结构和空 Native 库样本可能产生不同配置。最终 APK 的 ZIP 重写和复验由 `shield-core` 自己负责，apktool 配置只能作为辅助优化。

## 失败策略

原 APK 显式为 `false` 时，加固产物必须满足：

- 每个 `lib/**/*.so` 的压缩方法为 `Stored`。
- 每个 `.so` 的数据偏移是 16 KB 的整数倍。
- 不存在路径异常、重复同名条目或无法识别的 ABI 目录。
- 注入的 `libmocikashield.so` 已通过对应构建模式的 ELF 审计。

任一条件失败时，加固必须失败关闭并删除不合格输出，不能：

- 静默把 Manifest 改为 `extractNativeLibs=true`。
- 只记录警告后继续交付。
- 依赖用户签名时再次对齐来修复。

错误信息应说明“Native 库打包与 `extractNativeLibs=false` 不一致”，并列出有限数量的违规条目，不输出用户本地敏感路径。

## 16 KB 页大小关系

该缺陷与 16 KB 问题共享打包链，但不是同一个判断：

| 检查 | 解决的问题 |
|------|------------|
| ZIP `Stored` | 允许 `extractNativeLibs=false` 直接映射 `.so` |
| ZIP 16 KB 对齐 | 保证未压缩 `.so` 在 APK 内从合适边界开始 |
| ELF `LOAD` 段对齐 | 保证 Native 库内部加载段支持目标页大小 |

三项必须分别报告。现有 `zipalign -c -P 16 -v 4` 通过，只能证明 ZIP 偏移满足要求，不能证明 `.so` 未压缩，也不能证明 ELF 内部加载段兼容。

## 实施阶段

### 打包策略与核心修复（已完成）

- 增加 Manifest 三态解析及单元测试。
- 为 ZIP 重写增加内部 Native 库存储策略。
- 在加固流程中传递策略，原 APK 显式为 `false` 时强制全部 `.so` 不压缩。
- 保持 `true` 和未设置样本的原有压缩策略，记录加固前后体积变化。

### 产物验证与诊断（进行中）

- 扩展对齐验证结果，区分压缩方式和数据偏移错误。
- 将 Stub ELF 审计与发布资源检查关联，但不在每次普通文档 CI 中执行重型构建。
- 补充 `INSTALL_FAILED_INVALID_APK` 与 `Failed to extract native libraries` 的排障提示。

### 自动与设备回归（进行中）

- 扩展 Smoke 夹具，至少生成显式 `false` 的有 Native 库和无 Native 库两种输入。
- 断言加固后 Manifest 仍为 `false`，全部 `.so` 为 `Stored` 且 16 KB 对齐。
- 使用同一测试证书签名后执行真实设备安装与冷启动。
- API 23、API 28、API 35 16 KB 和 API 36 至少各验证一次；后续日常 PR 保留轻量 ZIP 结构测试，完整设备矩阵由候选版本执行。

## 版本规划

该问题会直接导致合法输入 APK 的加固产物无法安装，优先级为高，应纳入 `1.2.7` 正式版之前的下一候选版本。若 `1.2.7-rc.5` 尚未发布，则与每次启动安全检查修复一同进入 `1.2.7-rc.5`；若候选版本已经冻结，则顺延一个候选编号，不直接带入稳定版。

候选版本完成自动结构验证和至少一台真实设备安装启动后，可用于向反馈用户提供测试；稳定版仍需通过上述设备矩阵及现有 Android 4.4、ARouter、Android 9 和 16 KB 回归。

## 回滚原则

- 新策略只影响原 APK 显式为 `extractNativeLibs=false` 的加固路径。
- 出现体积或兼容回归时，可以回滚 ZIP 策略实现，但不得通过改写用户 Manifest 发布稳定版。
- 保留原始输入和失败输出的结构诊断数据，不保留测试证书或用户 APK。

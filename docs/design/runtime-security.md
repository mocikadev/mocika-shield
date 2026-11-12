# Android 运行时安全与 DEX 缓存演进设计

本文维护已交付的启动检查、DEX 缓存完整性和 Root 策略。内存 DEX 研究已暂停，具体格式见[技术内参](internals.md)，当前功能排期见[路线图](../process/roadmap.md)。

## 目标

- 安全检查成为每次进程启动的必经步骤，缓存命中不能绕过。
- 文件缓存中的明文 DEX 在加载前必须完成来源与完整性验证，并明确该验证不阻止 Root 环境读取文件。
- Root 环境策略由每次加固任务选择，同一份 Stub 资源支持不同策略。
- 文件路径保持默认；不将未达内存成本门槛的实验纳入正式产品。
- 保持 Android 4.4、ARouter、Android 9 共享库和 16 KB 页大小兼容性。

## 非目标与安全边界

- 不宣称 Root、内核提权或已完全控制应用进程的环境绝对安全。
- 不承诺阻止攻击者在代码实际执行后从进程内存抓取明文。
- 不因为新增 Root 策略而默认拒绝工控设备、模拟器和企业定制系统。
- 不在内存 DEX 调研完成前替换现有稳定文件加载路径。
- 本轮不重做 DEXB v5 加密协议；如后续需要变更格式，必须单独设计兼容与迁移方案。

正常零售设备上，应用私有目录仍是文件缓存的基础隔离边界；Root 环境下该隔离不能作为安全保证。运行时检测和缓存认证的目标是提高篡改、注入与提取成本，而不是建立不可绕过的可信执行环境。

## 已修复的设计缺口

早期 `Ld.extractDexFiles()` 在 `app_app_dex/v{versionCode}/` 缓存有效时直接返回文件列表；Native 反调试检查位于解密入口中，因此只有缓存未命中、真正进入解密时才执行。由此产生以下缺口：

1. 第二次及后续启动可以绕过启动期反调试检查。
2. 缓存有效性只检查目录、`.done` 和至少一个 DEX 文件，不能识别替换、缺失、多余或损坏文件。
3. 缓存只使用 `versionCode` 作为键；相同版本号覆盖安装不同 APK 时可能复用旧 DEX。
4. DEX 曾在写完后才设为只读，存在需要按 Android 14 动态代码加载要求收紧的写入窗口。
5. 原来没有 Root 检测及可选策略，现已增加标准／严格保护。
6. 解密后的完整业务 DEX 会长期保存在应用私有目录；普通应用无法跨沙箱读取，但 Root、可调试包的 `run-as`、注入或已攻破进程环境可能提取明文。

## 方案选择

| 方案 | 收益 | 风险 | 决定 |
|------|------|------|------|
| 一次实现全部能力 | 版本集中 | 跨 GUI、核心、Stub、缓存与类加载，难定位、难回滚 | 不采用 |
| 先切换内存 DEX | 立即减少高版本落盘 | 旧系统与缓存缺口仍存在，容易回归 Android 9 和 ARouter | 不采用 |
| 分阶段修复检查、认证缓存，并提前并行验证内存 DEX，随后接入 Root 策略 | 每阶段可独立验证、发布和回滚，尽早取得明文落盘风险的实测证据 | 交付周期较长 | 采用 |

## 职责与模块边界

当前职责结构：

```text
StubApp
  → RuntimeSecurity：读取策略并执行每次启动检查
  → DexCache：验证、创建、失效和清理文件缓存
  → Ld / Native：读取、解密和解压 DEX
  → DexInjector：选择文件或内存注入路径
  → 恢复真实 Application
```

职责边界：

| 单元 | 负责 | 不负责 |
|------|------|--------|
| `RuntimeSecurity` | 环境策略解析、每次启动检查编排 | DEX 缓存与类加载 |
| Native `environment_check` | ptrace、Frida、Root 高置信度信号检测 | GUI 策略和文件缓存 |
| `DexCache` | 缓存路径、完整性验证、原子写入、清理 | 加密算法与类注入 |
| `Ld` | JNI 声明、APK 内 payload 读取、Native 解密桥接 | 继续承载缓存和策略规则 |
| `DexInjector` | 按系统版本选择并执行注入 | 缓存生命周期和环境检测 |
| `shield-core` | 安全策略模型、Manifest 注入、缓存摘要生成 | Android 运行时检测实现 |
| Tauri / React | 固定枚举传递和用户提示 | 自行拼接 Manifest 字段或资源路径 |

不新增 crate。Java 新类型保持 package-private；Rust Native 检测保持模块内部，仅通过现有 JNI 注册边界暴露最小入口。

## 每次启动安全检查

新增独立 Native 检查入口，由 `StubApp.attachBaseContext()` 在读取缓存前调用：

```text
attachBaseContext
  → 读取环境策略
  → checkEnvironment()       每次启动必经
  → 验证缓存
  → 缓存命中：注入
  → 缓存失效：解密、写入、复验、注入
```

约束：

- 现有解密入口继续保留二次检测，防止其他调用路径直接绕过启动入口。
- 检测命中只返回通用短错误，不在日志中暴露具体规则。
- 单项检查读取失败不等于已 Root；兼容模式不得因此误杀。
- 环境检查无论文件缓存还是未来内存加载都必须执行。

## DEX 缓存认证

缓存认证解决的是来源、完整性与加载一致性，不提供保密性：能够读取应用私有目录的攻击者仍可读取认证通过的明文 DEX。不得在 GUI、README 或发布说明中把缓存摘要描述为“防提取”。明文暴露由环境策略和内存 DEX 路径分别降低风险。

### 不使用当前 DEXB 材料生成缓存 HMAC

当前 IKM、nonce 和证书指纹都可以从 APK 获得。直接使用这些材料派生缓存 HMAC 密钥，攻击者同样可以重新计算，不能形成独立的设备侧秘密。因此缓存认证采用“最终 APK 签名保护的预期摘要”，不引入表面安全但可重建的 HMAC。

### 预期摘要

加固时对原始 DEX 生成规范化根摘要：

```text
cache_root = SHA-256(
  schema_version
  + dex_count
  + index + name + size + SHA-256(dex)
  + ...
)
```

通过 Manifest 写入：

```xml
<meta-data android:name="rt.cache.schema" android:value="1" />
<meta-data android:name="rt.cache.count" android:value="3" />
<meta-data android:name="rt.cache.root" android:value="..." />
```

最终 APK 的 V2/V3 签名保护 Manifest；第三方修改摘要会破坏签名，修改后重新签名又会因证书指纹变化而无法解密。该方案不依赖 Android Keystore，能够统一覆盖 API 19～36。

### 缓存目录与验证

缓存目录调整为：

```text
app_app_dex/v{versionCode}-{cacheRootPrefix}/
```

摘要前缀只负责快速失效，完整根摘要才是安全判断。缓存命中必须同时满足：

- Schema 与 DEX 数量一致。
- 文件严格为 `c1.dex` 到 `cN.dex`，不存在缺失或额外 DEX。
- 规范路径位于当前缓存目录，拒绝符号链接和路径逃逸。
- 每个文件只读，大小与单文件摘要一致。
- 重新计算的规范化根摘要与 Manifest 一致。
- `.done` 标志在全部文件落盘并验证后最后创建。

允许缓存目录额外包含真实的 `oat/` 子目录，供 ART 保存优化产物；该目录不计入载荷文件数量，也不参与 DEX 根摘要。仍拒绝同名普通文件、顶层符号链接、未知目录和额外 DEX。优化产物由系统管理，允许其存在不代表本工具对其内容提供完整性认证。

Issue #117 暴露了旧版按目录项总数校验的缺陷：系统生成 `oat/` 后，未改变的 DEX 也会被误判失效，触发递归清理和重新解密。修复保持原缓存身份协议不变，加固工具更新后需重新加固 APK 才能使用新壳逻辑。缓存递归清理不得跟随符号链接删除目录外文件。

回归覆盖：只读缓存首次验证通过、加入模拟优化产物后连续验证仍通过、存在优化目录时篡改或缺失 DEX 仍被拒绝、额外 DEX 被拒绝，以及清理顶层和优化目录内部链接时保留外部文件。本地 JVM 测试验证缓存逻辑，不代替 Android 设备上的连续启动与 dex2oat 日志验证。

任一条件失败时删除整个缓存并重新解密；删除失败时失败关闭，不加载可疑缓存。

### 原子写入与只读要求

写入流程固定为：

```text
创建临时目录
  → 创建并打开 DEX 输出流
  → 立即将文件设为只读
  → 通过已打开的文件描述符写入
  → flush / sync / close
  → 计算并验证摘要
  → 创建 .done
  → 原子重命名为正式缓存目录
```

这样同时满足 Android 14 动态代码加载的只读要求，并避免把半写入文件识别为有效缓存。

## Root 环境策略

### 用户策略

首期只提供两个有明确运行意义的选项：

| GUI 文案 | 内部值 | 行为 |
|----------|--------|------|
| 标准保护（默认） | `compatible` | 保留现有反调试；Root 信号不阻止启动 |
| 严格保护 | `strict` | 反调试、注入或高置信度 Root 信号命中后拒绝解密 |

不提供只有日志、没有用户消费入口的“仅告警”模式。Android 4.4 工控兼容任务默认使用 `compatible`；用户主动选择严格模式时，GUI 必须提示 Root 工控系统、测试系统和模拟器可能被拒绝。

### 配置流

```text
React 加固页
  → ProtectRequest.environmentPolicy
  → ProtectExecution
  → shield_core::ProtectOptions
  → AndroidManifest meta-data
  → RuntimeSecurity 每次启动读取
```

Manifest 字段：

```xml
<meta-data
    android:name="rt.policy"
    android:value="strict" />
```

约束：

- GUI／共享入口可保存默认偏好；任务开始时解析有效值并固化，执行中不跟随偏好变化。
- 旧 GUI 请求缺少字段时默认 `compatible`。
- 当前协议新 Stub 读不到策略、类型错误或值非法时失败，不静默退回 `compatible`。
- 前端和 Tauri 只能传固定枚举，不得传任意 Manifest 值。
- CLI 使用同一核心枚举，默认值同样为 `compatible`。

### Root 信号分级

严格模式只根据高置信度组合阻止启动，例如明确可执行的 `su`、Magisk/Zygisk/KernelSU/APatch 注入或挂载痕迹、异常 UID，以及明确为 `service.adb.root=1` 的 Root ADB 等。`test-keys`、`userdebug/eng`、`ro.debuggable`、模拟器特征和单个可疑路径属于弱信号，不能单独触发阻止。

Native 内部可返回位标志用于自动化测试，但 Java 和业务日志只获得通用结果，避免暴露规则细节。

## 资源能力协议

`resources.zip/metadata.json` 增加显式能力：

```json
{
  "runtime_protocol": 4,
  "cache_schema": 1,
  "environment_policy": true,
  "memory_dex": false
}
```

当前标准／工控资源声明 `environment_policy: true`、`memory_dex: false`。协议 2 使用旧字段，协议 4 使用中性字段，详见[字段契约](manifest-runtime-contract.md)。旧资源不支持严格策略时由核心拒绝，不静默降级。

`shield-core` 在开始解包前检查资源能力：

- 选择严格策略但资源不支持时直接拒绝，不允许静默降级。
- 标准资源和 Android 4.4 兼容资源必须声明相同安全协议能力。
- 资源 metadata 解析改为类型化结构，不继续用字符串查找扩展新字段。
- 内存 DEX 未达到生产门槛前保持 `memory_dex: false`。

## 内存 DEX 研究结论

内存 DEX 未进入正式产品。原加载器搬运 Element 和直接初始化 DexPathList 的方案未达到兼容门槛；替换加载器的独立候选曾完成多组件、ARouter、多进程、覆盖安装与认证回退验证，但大 DEX 的长期内存成本未达到准入要求。

候选正式接入的实际最低边界为 API 31，不能沿用早期 API 26／29 探针范围作产品承诺。资源、状态协议和测量数据只在[候选设计](memory-dex-production.md)维护；标准／工控资源继续使用认证文件缓存，GUI 不开放内存选择，不为它预占版本号。

## 已交付与验收

每次启动检查随 1.2.7 收口，Native 别名、缓存认证和可选 Root 策略随 1.3.0 交付；后续中性 Manifest 字段随 1.4.2 更新。不能把早期版本方案表当作当前待办。

API 35 LineageOS 真机已验证普通 ADB 下严格策略正常，Root ADB 下严格策略拒绝、标准保护仍可启动；首次解密、缓存命中及篡改缓存后重建均通过。普通 ADB 读取不可调试包的私有目录被拒绝，不代表 Root 下不可提取。其他历史兼容证据见[测试清单](../process/test-checklist.md)。

## 验证矩阵

每个运行时阶段至少覆盖：

- API 19：Android 4.4 Dalvik。
- API 23：Android 6.0 ART。
- API 28：Android 9共享库。
- API 35：16 KB 页大小，`getconf PAGE_SIZE` 必须为 `16384`。
- API 36 真机。
- 单 DEX、多 DEX、自定义 Application、Native 库。
- ARouter 编译期插入和运行期扫描。
- 首次安装、冷启动、二次启动、清除数据、同签名覆盖安装。
- 相同 `versionCode`、不同 APK 覆盖安装。
- 缓存缺文件、多文件、字节篡改、权限异常和中断写入。
- 不可调试正式包通过普通 ADB 访问私有目录应失败；可调试包必须记录 `run-as` 提取边界。
- Root 文件管理器、ADB Root 与进程注入场景分别验证首次启动、二次启动、设备重启后的明文 DEX 暴露。
- 对提取到的 `cN.dex` 区分壳兼容类与完整业务类，不能只根据文件存在判断业务 DEX 已完整暴露。
- Issue #17 Android 9 Apache HTTP 样本。
- 标准资源和 Android 4.4 兼容资源。

Root 策略阶段额外覆盖普通真机误报与可控 Root 环境命中；内存 DEX阶段额外记录冷启动耗时、峰值内存和 GC 后延迟类加载。

## 回滚原则

- 完整阶段集中审查；是否发布候选包另行决定，不机械创建版本。
- 回滚以核心和配套资源为单位，不能为兼容跳过必要安全检查。
- 缓存认证异常时可删除缓存重建，但不得跳过验证继续加载。
- Root 严格策略出现兼容问题时，用户可重新以 `compatible` 加固；运行时不得自行静默降级。
- 内存 DEX 始终保留文件路径作为版本级回滚能力，未完成验证前不设为默认。

## 文档同步规则

- 本文维护方案、边界、阶段与验收门槛。
- [技术内参](internals.md) 只在能力真正落地后记录实际启动时序与格式。
- [GUI 设计](gui.md) 在 Root 选项实现时记录任务快照和枚举约束。
- [使用指南](../usage.md) 在用户可见选项发布时补充说明。
- [回归测试清单](../process/test-checklist.md) 维护持续执行的测试项。
- [路线图](../process/roadmap.md) 只保留当前状态和下一阶段摘要，不重复本文细节。

## 参考资料

- [Android 动态代码加载安全建议](https://developer.android.com/privacy-and-security/risks/dynamic-code-loading)
- [Android 14 更安全的动态代码加载要求](https://developer.android.com/about/versions/14/behavior-changes-14#safer-dynamic-code-loading)
- [InMemoryDexClassLoader API](https://developer.android.com/reference/dalvik/system/InMemoryDexClassLoader)

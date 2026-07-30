# Android 运行时安全与 DEX 缓存演进设计

本文定义 Mocika Shield Android Stub 的运行时安全边界、DEX 缓存认证、Root 环境策略与内存 DEX 调研计划。本文是后续实现和验收的主设计文档；具体格式落地后再同步到 [技术内参](internals.md)。

## 目标

- 安全检查成为每次进程启动的必经步骤，缓存命中不能绕过。
- 文件缓存中的明文 DEX 在加载前必须完成来源与完整性验证，并明确该验证不阻止 Root 环境读取文件。
- Root 环境策略由每次加固任务选择，同一份 Stub 资源支持不同策略。
- Android 8.0 以上逐步评估内存 DEX，减少持久明文落盘。
- 保持 Android 4.4、ARouter、Android 9 共享库和 16 KB 页大小兼容性。

## 非目标与安全边界

- 不宣称 Root、内核提权或已完全控制应用进程的环境绝对安全。
- 不承诺阻止攻击者在代码实际执行后从进程内存抓取明文。
- 不因为新增 Root 策略而默认拒绝工控设备、模拟器和企业定制系统。
- 不在内存 DEX 调研完成前替换现有稳定文件加载路径。
- 本轮不重做 DEXB v5 加密协议；如后续需要变更格式，必须单独设计兼容与迁移方案。

正常零售设备上，应用私有目录仍是文件缓存的基础隔离边界；Root 环境下该隔离不能作为安全保证。运行时检测和缓存认证的目标是提高篡改、注入与提取成本，而不是建立不可绕过的可信执行环境。

## 当前问题

当前 `Ld.extractDexFiles()` 在 `app_app_dex/v{versionCode}/` 缓存有效时直接返回文件列表；Native 反调试检查位于解密入口中，因此只有缓存未命中、真正进入解密时才执行。由此产生以下缺口：

1. 第二次及后续启动可以绕过启动期反调试检查。
2. 缓存有效性只检查目录、`.done` 和至少一个 DEX 文件，不能识别替换、缺失、多余或损坏文件。
3. 缓存只使用 `versionCode` 作为键；相同版本号覆盖安装不同 APK 时可能复用旧 DEX。
4. DEX 当前在写完后才设为只读，存在需要按 Android 14 动态代码加载要求收紧的写入窗口。
5. 没有 Root 环境检测，也没有面向不同设备类型的可选策略。
6. 解密后的完整业务 DEX 会长期保存在应用私有目录；普通应用无法跨沙箱读取，但 Root、可调试包的 `run-as`、注入或已攻破进程环境可能提取明文。

## 方案选择

| 方案 | 收益 | 风险 | 决定 |
|------|------|------|------|
| 一次实现全部能力 | 版本集中 | 跨 GUI、核心、Stub、缓存与类加载，难定位、难回滚 | 不采用 |
| 先切换内存 DEX | 立即减少高版本落盘 | 旧系统与缓存缺口仍存在，容易回归 Android 9 和 ARouter | 不采用 |
| 分阶段修复检查、认证缓存，并提前并行验证内存 DEX，随后接入 Root 策略 | 每阶段可独立验证、发布和回滚，尽早取得明文落盘风险的实测证据 | 交付周期较长 | 采用 |

## 职责与模块边界

目标启动结构：

```text
StubApp
  → RuntimeSecurity：读取策略并执行每次启动检查
  → DexCache：验证、创建、失效和清理文件缓存
  → Ld / Native：读取、解密和解压 DEX
  → DexInjector：选择文件或内存注入路径
  → 恢复真实 Application
```

建议边界：

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
<meta-data android:name="dev.mocika.shield.CACHE_SCHEMA" android:value="1" />
<meta-data android:name="dev.mocika.shield.CACHE_DEX_COUNT" android:value="3" />
<meta-data android:name="dev.mocika.shield.CACHE_ROOT_SHA256" android:value="..." />
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
| 兼容模式（默认） | `compatible` | 保留现有反调试；Root 信号不阻止启动 |
| 严格环境保护 | `strict` | 反调试、注入或高置信度 Root 信号命中后拒绝解密 |

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
    android:name="dev.mocika.shield.ENV_POLICY"
    android:value="strict" />
```

约束：

- 该选择属于当前加固任务，任务开始时固化，不写入全局 `config.toml`。
- 旧 GUI 请求缺少字段时默认 `compatible`。
- 新 Stub 读取不到字段时默认 `compatible`。
- 前端和 Tauri 只能传固定枚举，不得传任意 Manifest 值。
- CLI 使用同一核心枚举，默认值同样为 `compatible`。

### Root 信号分级

严格模式只根据高置信度组合阻止启动，例如明确可执行的 `su`、Magisk/Zygisk/KernelSU/APatch 注入或挂载痕迹、异常 UID，以及明确为 `service.adb.root=1` 的 Root ADB 等。`test-keys`、`userdebug/eng`、`ro.debuggable`、模拟器特征和单个可疑路径属于弱信号，不能单独触发阻止。

Native 内部可返回位标志用于自动化测试，但 Java 和业务日志只获得通用结果，避免暴露规则细节。

## 资源能力协议

`resources.zip/metadata.json` 增加显式能力：

```json
{
  "runtime_protocol": 2,
  "cache_schema": 1,
  "environment_policy": true,
  "memory_dex": false
}
```

`environment_policy` 在 `1.3.0-alpha.1` 保持 `false`；Root 策略、清单配置和 GUI 任务选项接通后改为 `true`。旧资源选择严格模式时由核心直接拒绝，不得静默降级。

`shield-core` 在开始解包前检查资源能力：

- 选择严格策略但资源不支持时直接拒绝，不允许静默降级。
- 标准资源和 Android 4.4 兼容资源必须声明相同安全协议能力。
- 资源 metadata 解析改为类型化结构，不继续用字符串查找扩展新字段。
- 内存 DEX 未达到生产门槛前保持 `memory_dex: false`。

## Android 8.0 以上内存 DEX 调研

`InMemoryDexClassLoader` 从 API 26 提供；API 26 只有单 `ByteBuffer` 构造方式，API 27 才支持 `ByteBuffer[]` 多 DEX。因此内存方案必须按版本验证，不能直接用一条 API 26+ 分支替换现有路径。

候选范围：

| API | 调研方向 |
|-----|----------|
| 19～25 | 保持经过验证的私有文件缓存 |
| 26 | 单 DEX内存加载；多 DEX单独验证 Loader/Element 组合 |
| 27 | 多 `ByteBuffer` 内存 Loader |
| 28 | 在多 DEX基础上回归 Apache HTTP 系统共享库优先级 |
| 29+ | 作为首个生产候选范围，验证后再向下扩展 |

调研必须回答：

- 内存 Element 能否按原顺序前插到应用 `PathClassLoader`。
- 多 DEX 跨包引用、真实 Application、ARouter 是否正常。
- `ByteBuffer` 生命周期和垃圾回收后是否仍能加载尚未访问的类。
- 原 APK Native 库搜索路径是否保持不变。
- API 28 共享库兼容处理如何扫描内存 DEX 类名。
- 每次启动解密解压带来的冷启动耗时和内存峰值是否可接受。
- API 35 16 KB 与 API 36 真机是否稳定。

首个实验路径和生产候选都只考虑 API 29+。实验实现安排在 `1.3.0-alpha.2`，默认关闭且不替换正式文件路径；API 26～28 在专项证据充分前继续走文件路径。实验未达到门槛时不得阻塞 `1.3.0`，也不得以未验证状态进入正式资源。

### 原加载器内存注入实验结论

2026-07-29 在 API 35 真机使用双 DEX、真实 Application 和同签名覆盖安装夹具完成了两条原型验证：

1. 通过公开 `InMemoryDexClassLoader` 创建内存 Element，再前插到应用原 `PathClassLoader`。ART 在首次定义业务类时拒绝同一 DEX 被登记到两个 ClassLoader，启动失败。
2. 尝试直接让应用原 `DexPathList` 初始化内存 DEX，避免临时加载器绑定。该能力依赖非 SDK 的 `initByteBufferDexPath` 或 `DexFile` 内部构造器；当前真机无法稳定反射访问，不能作为产品实现基础。

因此本阶段不合入内存加载代码，不增加 Manifest 开关，`metadata.json` 保持 `memory_dex: false`。若后续继续研究，只能独立评估“替换应用 ClassLoader”方案；该方案会改变既有唯一加载器边界，必须重新验证 ARouter、Native 库查找、系统共享库、组件实例化和框架 ClassLoader 引用，不得直接复用本阶段结论进入正式资源。

### 替换应用 ClassLoader 的候选边界

`1.4.0` 不再尝试把由 `InMemoryDexClassLoader` 创建的 Element 搬运到原 `PathClassLoader`，也不依赖非 SDK 的内存 DexFile 构造器。候选方案是在壳 `Application.attachBaseContext()` 内完成解密后创建唯一的业务 `InMemoryDexClassLoader`，并在任何业务组件实例化前替换当前 `LoadedApk.mClassLoader`。业务 Application、Provider、Activity、Service 和 Receiver 后续都必须由该加载器定义。

加载器关系固定为：

```text
BootClassLoader
├── 原 PathClassLoader：只负责启动壳和 APK 中保留的静态类
└── InMemoryDexClassLoader：负责全部业务 DEX，并继承原应用 Native 搜索目录
```

反射替换路径中，业务加载器的 parent 必须是原 `PathClassLoader` 的 parent，不能把原加载器作为 parent。否则原 APK 中尚未抽离或重复存在的业务类会被父优先委派提前定义，重新形成双加载器类身份。使用 `AppComponentFactory` 公开入口时存在不同约束：工厂类和壳 Application 已由系统默认加载器定义，返回的业务加载器需要以默认加载器为 parent 才能继续实例化壳组件。因此正式资源必须保证默认加载器只包含壳类、业务 DEX 只存在于内存载荷，两者类集合不交叉；无法稳定分离时停止该方案。

`InMemoryDexClassLoader` 的 `librarySearchPath` 必须继承原 APK 的完整 Native 搜索语义。只传 `ApplicationInfo.nativeLibraryDir` 无法覆盖 `extractNativeLibs=false` 时直接从 APK 加载 `.so` 的路径；至少需要包含 `sourceDir!/lib/<ABI>`、应用 Native 目录和系统公开库路径，并验证任务级别名壳库与原业务库均可加载。路径无法从公开稳定信息重建时，不允许接入生产资源。

框架引用至少包括：

- 当前包 `LoadedApk.mClassLoader`
- `ContextImpl.mPackageInfo` 指向的同一 `LoadedApk`
- 当前线程的 context ClassLoader
- 真实 Application 创建后已有的 `mInitialApplication`、`mAllApplications`、`LoadedApk.mApplication`
- 多进程场景中每个进程独立执行的相同替换时序

`mDefaultClassLoader`、`AppComponentFactory`、split ClassLoader、Instrumentation 和厂商框架缓存是否需要同步，必须通过真实样本和 AOSP 版本差异调查决定，不能仅靠设置 `mClassLoader` 推定兼容。

#### 回退边界

替换加载器不是运行期可随时切换的开关：

1. 解密、ByteBuffer 构造、加载器创建或框架引用写入失败，并且尚未定义任何业务类时，可以放弃候选加载器并重启到文件路径模式。
2. 一旦真实 Application、Provider 或其他业务类已经由内存加载器定义，禁止在同一进程静默切回文件加载器；这会产生重复类身份和部分初始化状态。
3. 首期原型不实现自动重启回退。生产接入前应使用持久化的“下次启动使用文件模式”状态，并由独立进程重启完成回退；状态必须认证且不能绕过缓存安全检查。
4. API 28 及以下始终保持现有认证文件缓存，不参与内存路径失败判断。

#### 隔离探针结果

2026-07-30 在 API 35 真机运行 `tests/scripts/run-memory-loader-probe.sh`：主进程和远程 Service 进程分别完成加载器替换；业务 Application、Provider、Activity、Service 和第二个 DEX 中的跨包类均由替换后的 `InMemoryDexClassLoader` 正常创建；主动触发 GC 后，第二个 DEX 中此前未访问的类仍可延迟加载；`extractNativeLibs=false` 场景下业务类可从 APK 内加载 Native 库并执行 JNI；应用私有目录未生成 DEX 文件。该探针与正式 Stub、DEXB v5、GUI 和资源包完全隔离。

这只证明最小框架链路可行。以下项目仍是进入正式资源前的阻塞项：

- 任务级别名壳库与正式 Stub Native 初始化
- ARouter 编译期插入与运行期扫描
- Android 9 系统共享库和 AppComponentFactory
- split APK、Instrumentation 和厂商 ClassLoader
- 首次安装、清除数据、同签名覆盖安装和崩溃后文件模式重启回退
- API 29、API 35 16 KB、API 36 的性能和内存矩阵

探针通过不改变正式能力：`metadata.json` 继续保持 `memory_dex: false`，标准/API 19 资源与 GUI 均不接入该路径。

#### `AppComponentFactory` 与反射入口对照

2026-07-30 在同一 API 35 真机和同一组双 DEX 载荷上，将隔离探针拆成两个构建变体：

1. 反射变体继续在壳 Application 的 `attachBaseContext()` 中写入 `LoadedApk.mClassLoader`。
2. 工厂变体通过公开的 `AppComponentFactory.instantiateClassLoader()` 返回业务 `InMemoryDexClassLoader`，不反射修改框架 ClassLoader 字段。

两种变体均在主进程和远程 Service 进程通过真实 Application、Provider、Activity、Service、跨 DEX 引用、APK 内 Native 库、GC 后延迟首次加载和私有目录无 DEX 检查。公开工厂入口由系统在 Application Context 初始化和任何应用组件实例化前调用，加载器时序与框架契约更明确，因此作为下一阶段首选；反射路径只保留为对照和止损依据，不进入正式实现。

公开入口最初识别出两个生产阻塞项：

- 回调只有默认 ClassLoader 与 `ApplicationInfo`，尚无可用 `Context`。当前 DEXB v5 解密必须通过 `Context` 读取设备实际签名，不能原样前移；必须先设计不降低签名绑定强度的早期证书读取边界。
- 原应用可能声明 AndroidX 或自定义 `AppComponentFactory`。壳工厂不能直接覆盖其 Application、Activity、Service、Receiver 和 Provider 实例化语义；必须验证加载业务 DEX 后的安全委托方案，并处理委托工厂创建失败和递归配置。

2026-07-30 已在工厂变体中增加载荷侧自定义工厂：壳工厂创建业务加载器后，由该加载器创建原工厂；真实 Application 由壳 Application 主动通过原工厂创建，Provider、Activity、Receiver 和远程进程 Service 则由系统回调壳工厂后转发。五类组件的原工厂标记与实际生命周期标记均通过，证明委托机制在最小框架链路中可行。

正式接入仍需把原工厂类名以独立元数据保存，区分“原应用未声明工厂”、AndroidX 工厂、自定义工厂和错误地指回壳工厂的递归配置；委托创建失败必须失败关闭，不能静默退回默认工厂。无 `Context` 阶段的签名绑定解密仍是进入正式 Stub 前的主要阻塞项。在这些边界完成前，不开放 `memory_dex` 能力字段。

## 任务阶段与版本规划

### `1.2.7-rc.5`：修复启动检查绕过

范围：

- 抽出独立环境检查入口。
- 每次启动在缓存判断前执行反调试检查。
- 解密入口保留纵深检查。
- 不增加 Root GUI 选项，不改变缓存格式。

验收：API 19、23、28、35 16 KB 与 API 36 首次/二次启动；ARouter 与 Issue #17 样本不回归。该修复通过后与 Android 4.4 真机结果一起决定 `1.2.7` 稳定版发布时间。

### `1.2.7`：当前兼容主线正式版

范围：不再增加运行时安全能力，只纳入已通过验证的启动检查修复和 Android 4.4、ARouter、Android 9、16 KB 兼容改进。

发布门槛：候选版本的设备矩阵、用户验证和三平台发布构建全部通过。Android 4.4.2 `armeabi-v7a`/NEON 工控真机已经确认核心运行正常，未逐项覆盖的特定业务场景不再作为当前发布阻塞项。

### `1.3.0-alpha.1`：Native 去品牌化与认证缓存

Native 别名任务范围：按独立设计完成任务级中性别名、Stub DEX 等长替换、四 ABI 资源映射和冲突检测。该任务不同时修改缓存格式、Root 策略或 DEX 加载路径。

Native 库别名完成后继续实现认证缓存：

- 新增缓存根摘要和 Manifest 字段。
- 拆出 `DexCache`，实现严格验证与原子重建。
- 修复相同 `versionCode` 覆盖安装复用旧缓存。
- 修正 Android 14 只读写入顺序。
- 增加 `runtime_protocol` 与 `cache_schema` 能力检查。

统一验收：Native 别名的标准/API 19 双资源、四 ABI、16 KB、ARouter 和覆盖安装通过；缓存缺失、多余、字节篡改、权限异常、相同版本覆盖安装均可预测处理；所有现有系统路径完成回归，资源协议完成集成验证。详细别名协议见 [Native 库名称去品牌化与按任务别名设计](native-library-alias.md)。

当前实现状态：缓存根摘要、Manifest 身份、严格目录校验、只读落盘、原子替换和资源能力协议已经实现；JVM 回归覆盖完整缓存、字节篡改、DEX 缺失、多余文件、缺少完成标记和删除失败。标准资源已在 Android 真机完成未加固基线、同签名覆盖安装、首次启动和缓存命中后二次启动。标准/API 19 双资源均已完成构建和 API 19 Native 审计；由于当前新版模拟器不再支持既有 `armeabi-v7a` API 19 镜像，本轮尚未重复执行 Dalvik 运行回归，进入 `alpha.1` 前仍需在可用低版本环境补测。

### `1.3.0-alpha.2`：Root 策略与内存 DEX 实验

Root 策略范围：

- 实现高置信度 Root/Magisk/Zygisk 检测。
- 接入 `compatible/strict` 枚举、Manifest 配置流和 GUI 任务级选项。
- CLI 与旧请求保持默认兼容。

当前实现状态：核心固定枚举、资源能力握手、Manifest 配置、GUI 任务级选项及 Native 高置信 Root 信号已接通；兼容模式维持原有反调试行为，严格模式额外拒绝 Root 信号。标准/API 19 双资源构建完成。API 35 普通真机已通过严格模式双 DEX、真实 Application、同签名覆盖安装、首次启动和二次启动验证；同一台 LineageOS 开启 Root ADB 后，严格模式按 `service.adb.root=1` 正确拒绝启动，兼容模式仍可完成首次和二次启动。

内存 DEX 实验范围：

- 在 `DexInjector` 内增加仅供开发验证的 API 29 以上内存加载路径。
- 默认关闭，不进入正式 GUI，不替换文件加载默认路径。
- 建立文件加载与内存加载的同一套样本、性能和提取场景对照数据。
- 不在本阶段扩展 API 26～28，也不引入 DEX 结构破坏或反编译干扰。

当前实验结论：在保持应用原 `PathClassLoader` 的约束下，公开加载器搬运 Element 会被 ART 拒绝，直接初始化原 `DexPathList` 又依赖不可稳定访问的非 SDK 接口。本阶段按未达门槛处理，不合入运行代码、不进入 GUI，正式资源继续声明关闭。

验收：多 DEX、真实 Application、ARouter、Android 9 共享库、Native 库、16 KB、垃圾回收后延迟类加载全部正常；记录冷启动耗时、峰值内存，并验证运行期间与进程退出后是否仍产生可提取的完整 DEX 文件。未达门槛时保留实验报告并关闭路径，不阻塞后续 `1.3.0`。

### `1.3.0-beta.1`：功能冻结与完整回归

范围：不再增加新能力或修改资源协议。Stub DEX 指标基线和低风险静态清理已经完成，本阶段集中验证 Native 别名、认证文件缓存与 Root 策略。内存 DEX 实验已在 Alpha 阶段关闭，不作为本阶段运行路径或验收项。

验收：普通真机严格模式不误报；可控 Root 环境能够命中；Android 4.4 工控兼容模式默认不被阻断；真实 APK、设备矩阵和性能回归通过。发布说明必须明确严格模式只能提高提取成本，不能承诺抵御隐藏 Root、检测绕过或进程内 Dump。

当前验收结果：核心 95 项、Native 15 项、Stub JVM 22 项、维护脚本 19 项测试全部通过，前端生产构建、标准/API 19 双资源构建、Stub DEX 指标守门和无设备端到端加固回归通过。API 35 LineageOS 真机在普通 ADB 下严格策略未误报；开启 ADB Root 后严格策略正确拒绝，兼容策略仍通过首次解密和缓存命中启动。Root 下篡改 `c1.dex` 后，下一次冷启动自动废弃并重建完整缓存，双 DEX 与真实 Application 正常；普通 ADB 无法读取私有缓存目录。缓存命中后的三次冷启动耗时为 177～185 毫秒。Android 4.4、6.0、9、16 KB 和真实用户样本继续引用 `1.2.7` 收尾审计中未被本阶段改动触及的有效回归证据。

### `1.3.0-rc.1`：完整安全回归

范围：只修复 alpha/beta 验证发现的问题，不再扩展安全信号和 UI 范围。完成三平台 GUI 打包、标准/API 19 双资源和全设备矩阵验证后，再决定 `1.3.0` 稳定版。内存 DEX 只有在实验门槛全部通过时才允许保留为实验能力，否则从候选资源关闭。

### `1.3.0`：缓存安全与环境策略正式版

正式交付 Native 库名去品牌化、缓存完整性、原子缓存重建、资源能力协议、Root 环境策略和 Stub DEX 低风险静态治理。API 29 以上内存 DEX 不作为该正式版的承诺能力；即使实验结果良好，也默认关闭并继续积累生产候选证据。

### `1.4.0-alpha.1`：内存 DEX 实验候选

分成两个独立门禁：先用隔离探针验证替换 `LoadedApk` 应用 ClassLoader 的最小框架链路；再扩展到真实加固链路。最小探针不得进入正式 Stub、GUI 或资源包。只有 ARouter、Native、Android 9、split、多进程、覆盖安装、GC 延迟加载和跨版本矩阵全部达到门槛后，才允许把候选实现接入开发资源并发布 `alpha.1`。

### `1.4.0-beta.1`：内存 DEX 扩大验证

在受控开关下扩大设备、厂商系统和真实应用样本，验证升级覆盖、回退文件路径、崩溃恢复、性能与提取风险。不得为了扩大覆盖范围把 API 26～28 自动纳入。

### `1.4.0-rc.1`：内存加载发布冻结

冻结默认启用范围、回退条件和用户说明，只修复候选验证发现的问题。完整执行文件/内存双路径、Root 策略、标准/API 19 双资源和三平台发布回归。

### `1.4.0`：内存 DEX 正式版

仅在生产候选门槛全部通过后，对已验证的 API 29 以上范围正式启用内存 DEX；其他系统继续使用认证文件缓存。若门槛未通过，则继续发布候选修复，不以版本计划倒逼正式启用。

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

- 每个阶段独立 PR 和候选版本，不跨阶段混合提交。
- 启动检查修复可回滚到旧调用时序，不改变 APK 协议。
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

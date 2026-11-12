# 技术内参

更新：2026-11-12。本文记录当前标准／工控文件加载路径与 DEXB v5 格式；源码类名只用于定位，不代表 APK 中的最终混淆名。模块分工见[架构](architecture.md)，操作步骤见[使用指南](../usage.md)。

## 整体流程

```text
输入 APK 验签与预检
  → apktool 解包（不反编译 Smali）
  → 修改 Manifest、收集业务 DEX 与 ARouter 路由入口
  → DEX 自然序排列、Zstd 压缩、认证加密
  → 注入匹配的壳 DEX 和 Native 库
  → 在 classes.dex 尾部追加 MSHD／DEXB 载荷
  → 重打包、ZIP 对齐
  → 使用输入 APK 的同一证书签名
```

普通纯加固输出尚未签名；加固后自动签名和受控任务按各自契约交付。GUI、CLI 共用核心，受控父进程额外管理期限、停止确认及最终无覆盖提交。

## 输入、签名与预检

- 只接受实际验签成功的单内容签名 APK；不能只看 `META-INF` 判断 V2／V3 签名是否存在。
- 提取当前 X.509 内容签名证书 DER 的 SHA-256，规范为大写 64 位十六进制。不使用 public key digest、Source Stamp 或失效的 V1 残留。
- 输入已经有本项目载荷时拒绝二次加固；多内容签名、无法读取指纹或自动签名证书不一致时阻断，不猜测证书。
- `preflight` 是有限结构／签名检查，不证明所有业务库、Manifest 语义、ELF 页大小或设备运行均兼容。
- `doctor` 检查依赖就绪，不能代替真实私钥签名；完整 Agent 协议见[命令契约](agent-cli-contract.md)。

## APK 修改与资源

### Manifest

核心根据资源 `runtime_protocol` 选择字段集合，写入实际壳 Application、原 Application、环境策略和缓存身份。当前标准／工控协议 4 使用中性字段 `rt.*`；旧协议 2 保留旧字段。新壳必要字段缺失、类型错误或策略非法时明确失败，不默认降级。

原包占用当前协议的 Application 直属保留字段时，在写入前拒绝；其他组件元数据和业务字符串不作全文替换。原组件工厂是否移除或由候选代理保存委托，取决于资源声明；不能将内部候选行为当作正式路径。字段定义及兼容矩阵见[Manifest 契约](manifest-runtime-contract.md)。

### DEX 与 Native

业务 DEX 按 `classes.dex`、`classes2.dex`、…、`classes10.dex` 的自然序处理，不按字典序。每份以 Zstd level 19 压缩，元数据和压缩内容整体认证加密。

资源 ZIP 包含 `metadata.json`、`stub-classes.dex` 及四种主流 ABI 的规范 Native 库。规范名 `libmocikashield.so` 是资源内部契约；注入 APK 时按任务选择不冲突的别名，并联动改写壳 DEX。因此不能按固定库名识别输出。

构建从同批 R8 映射生成 JNI 类名、方法名、签名和回调绑定。业务类与壳完整类名冲突时停止；不通过保留所有原始类名规避冲突。详见[库名别名](native-library-alias.md)、[壳身份](stub-identity.md)和[Native 暴露面](native-surface.md)。

### 资源与重打包

`extractNativeLibs=false` 必须保留，全部 `lib/**/*.so` 使用 ZIP Stored 且按 16 KB 对齐；其他普通 Stored 条目按 4 字节对齐。ZIP 偏移、压缩方式和 ELF LOAD 段页大小是三项不同检查，不能互相替代。

解包后只对文件头确认是 JPEG、扩展名却为 `.png` 的资源无损改名；九宫格或同名 `.jpg` 冲突明确失败，不转码、不覆盖。详细规则见[Native 打包](native-library-packaging.md)与[处理基线](apk-processing-baseline.md)。

## 加密数据格式（DEXB v5）

载荷位于壳 `classes.dex` 的 DEX `file_size` 之后：

```text
[标准 DEX 内容]
MSHD          4 字节 magic
payload_len   u32 小端，仅计算后续 DEXB 数据
DEXB 数据     payload_len 字节
```

MSHD 位置须同时满足 magic、长度及文件末尾一致；预检流式扫描，不依赖固定尾部窗口。追加前按原 DEX header 边界裁剪，避免重复追加。常规反编译工具可能不展示尾部，但它仍可被发现和提取。

DEXB 头部均为明文：

| 顺序 | 长度 | 字段 |
|---|---|---|
| 1 | 4 | magic：`DEXB` |
| 2 | 4 | version：u32 小端 5 |
| 3 | 4 | dex_count：u32 小端 |
| 4 | 1 | sig_len |
| 5 | sig_len | 证书 SHA-256 的十六进制 ASCII，正式流程为 64 字节 |
| 6 | 1 | ikm_len |
| 7 | ikm_len | 每次加固随机 IKM，当前 32 字节 |
| 8 | 12 | 随机 nonce |
| 9 | 剩余 | ChaCha20-Poly1305 密文，尾部含 16 字节认证标签 |

密文解开后先为 N 份元数据，再为 N 份对应压缩数据：

```text
每份元数据：
name_len(u8) + name + compressed_size(u32小端) + original_size(u32小端)
随后：
compressed_data[1] + ... + compressed_data[N]
```

Stub 只接受 v5，并对数量、长度、格式和认证结果检查；v4 不兼容，旧 APK 须重新加固。DEXB 版本与资源协议、缓存 schema、CLI 配置及事件版本相互独立。

## 密钥派生与安全边界

```text
key = HKDF-SHA256(
  IKM = 头部随机 IKM,
  salt = 12 字节 nonce,
  info = 当前签名证书 SHA-256 的 ASCII
) → 32 字节
ciphertext = ChaCha20-Poly1305(key, nonce, 元数据与压缩数据)
```

运行时读取自身实际签名指纹参与派生，并执行常数时间比较；重签名会改变派生结果，普通修改也会被认证校验发现。指纹读取需要显式传入 attachBaseContext 的 Context，不能依赖尚未建立的全局 Application。

IKM、nonce 和证书指纹不是设备独有秘密，签名绑定也不等于攻击者无法分析、重建或转储明文。该设计提高静态提取和未授权重打包成本，不构成 Root／已控进程上的可信执行环境，不承诺“无法脱壳”。

## Android 启动与加载

标准／工控路径：

```text
StubApp.attachBaseContext
  → 每次启动读取并检查环境策略
  → 验证缓存，失效则重新解密和原子重建
  → 向原应用 ClassLoader 注入业务 DEX
  → 创建并 attach 真实 Application
StubApp.onCreate
  → 替换必要的框架 Application 引用
  → 准备当前 ARouter 路由快照
  → 调用真实 Application.onCreate
```

缓存命中也先检查环境，解密入口保留纵深检测。缓存目录身份包含应用版本与 DEX 根摘要；只读、数量、名称、路径、内容和完成标志全部符合后才加载，允许系统真实 `oat/` 目录。认证不阻止有权限者读取明文，详见[运行时安全](runtime-security.md)。

| Android 范围 | 文件 DEX 注入 |
|---|---|
| API 19～20 | 工控资源的 Dalvik 路径，实际 4.4 限 v7a／NEON |
| API 21～22 | `makeDexElements` 工厂 |
| API 23 | `makePathElements` 工厂 |
| API 24 及以上 | 优先 Native 调用 `addDexPath`，失败按既有 Java 回退 |

各路径保持业务类的加载器身份；Android 9 声明的系统共享库须优先于业务编译桩。JNI 和反射涉及内部实现，不能宣称天然不受非 SDK 限制或无需逐版本验证。兼容证据见[4.4 设计](android-4.4-compatibility.md)与[测试清单](../process/test-checklist.md)。

Native 通过 `JNI_OnLoad`／`RegisterNatives` 注册 DEX 注入、解密、环境检查及内部候选缓冲接口，最终名称／签名由构建映射决定。资源只导出允许入口，不等于接口无法被分析。

## ARouter 兼容

加固阶段从业务 DEX 收集 Root、Providers、Interceptors 注册入口，形成 `arouter_routes.txt`。不存在 ARouter 或没有路由清单时不做补偿。

在真实 Application 启动前，用当前 APK 的路由集合和版本同步替换 `SP_AROUTER_CACHE`，由 ARouter 正常初始化流程消费，避免旧加固方案遗留缓存和运行期扫描不到加密 DEX。只对可调试包保留提前反射注册，因为这类包会忽略版本缓存并强制扫描；不能将它描述为所有正式 APK 都手动注册。

## 已修复问题与继续维护的风险

| 问题 | 当前处理 |
|---|---|
| V2／V3-only 签名无法提取、错误降级 | 统一有效内容签名来源，提取失败阻断 |
| JNI 回调被裁剪或 Context 为空 | 完整保留回调成员、同批映射、显式传 Context |
| 大载荷预检漏报、重复加固 | 流式扫描完整追加块，核心再次拒绝已加固 |
| ARouter 首次扫描／覆盖安装旧缓存 | 当前路由快照、同步版本缓存及调试包特例 |
| Android 9 共享库编译桩被优先加载 | 保持系统共享库解析优先 |
| 缓存命中绕过检查、缓存篡改 | 每次启动检查、签名保护的根摘要、整体失效重建 |
| `extractNativeLibs=false` 安装失败 | 保留字段、Stored Native、ZIP 16 KB 对齐 |

内部 API、厂商系统、Root 误判和真实业务兼容仍需回归。内存 DEX 候选因长期内存成本暂停；方法代码分离已停止产品接入，不能根据研究原型宣称正式支持。当前计划只以[路线图](../process/roadmap.md)为准。

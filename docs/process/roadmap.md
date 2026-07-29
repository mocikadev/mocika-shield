# ROADMAP.md — Mocika Shield 功能路线图

> 记录待修复缺陷与待实现功能。技术细节见 [internals.md](../design/internals.md)。
> 最后更新：2026-07-29

---

## 说明

- 状态：`待修复` / `待实现` / `进行中` / `已完成` / `已否决`
- 优先级：`高` / `中` / `低`

---

## 正式开源后的演进方向

`v1.2.0` 之后优先保持主线稳定，避免把大规模重构、保护能力增强和 GUI 交互调整混在同一轮迭代中。后续演进按以下阶段推进：

| 阶段 | 目标 | 重点任务 | 版本规划 |
|------|------|----------|----------|
| 当前兼容主线 | 固化已经验证的低版本与运行时兼容能力 | 每次启动安全检查、Android 4.4、ARouter、Android 9、16 KB 回归 | 已完成：`1.2.7` |
| 缓存安全与环境策略 | 区分缓存完整性和明文保密性，提供可选择的运行环境策略 | Native 库名去品牌化、缓存摘要、原子重建、资源能力协议、API 29+ 内存 DEX 实验、Root 兼容/严格策略、Stub DEX 静态治理 | `1.3.0-alpha.1` → `alpha.2` → `beta.1` → `rc.1` → `1.3.0` |
| 内存 DEX 生产化 | 在兼容和性能证据充分后减少高版本完整明文 DEX 持久落盘 | API 29+ 生产候选、文件路径回退、扩大设备验证、发布冻结 | `1.4.0-alpha.1` → `beta.1` → `rc.1` → `1.4.0` |
| 用户诊断与兼容性预检 | 让普通用户在加固前发现风险，遇到问题时能提供有效反馈 | 错误分类、本地加固报告、minSdk/targetSdk、ABI、split APK、已有壳和签名方案预检 | 低风险静态预检可独立插入，完整诊断体系待排期 |
| 自动化与批处理 | 支持更高频、重复性使用场景 | GUI 批量加固、CLI 配置文件、CI 集成模式、可复用任务模板 | 待排期 |
| 后续保护增强 | 减少默认壳暴露面，并让提取单一载体或快照后不能直接得到完整业务 DEX | Stub DEX 能力变体、可选二阶段加载实验、DEX 结构与方法代码分离、内存重建、分片恢复可行性 | Stub 最小化可独立推进；深层保护在 `1.4.0` 后调研，不预占版本号 |
| 供应链与发布成熟度 | 提升开源用户信任与正式分发体验 | 安装包代码签名、公证、SBOM、发布证明、依赖审计自动化 | 持续推进 |

### 迭代原则

- `main` 保持可发布，复杂功能使用临时 `feat/*` 或 `fix/*` 分支开发，合并后删除
- 当前不引入长期 `develop` 分支；只有需要维护多个正式版本线时再开 `release/x.y`
- 每个 minor 版本聚焦一个主题，避免同时引入大重构和大功能
- 保护算法、壳加载、签名链路、证书数据库这类高风险改动必须配套回归测试或可复现手测清单
- GUI 继续只维护 Tauri 版本，不恢复 SwiftUI / GNOME / 其他 GUI 实现

### 预发布阶段定义

| 阶段 | 进入条件 | 允许的变更 | 退出条件 |
|------|----------|------------|----------|
| Alpha | 版本主题和边界已经确定，核心能力仍在建设 | 可增加计划内能力、调整内部协议和默认关闭的实验路径 | 计划交付能力全部完成，协议和 GUI 交互具备冻结条件 |
| Beta | 版本承诺范围完整，功能和协议冻结 | 兼容性、性能、静态治理和缺陷修复，不再增加能力 | 真实 APK 与设备矩阵通过，无已知高优先级回归 |
| RC | 发布产物、配置格式和默认行为冻结 | 只修复阻塞正式发布的问题 | 三平台发布构建及完整回归通过，候选产物可原样转正式版 |
| 正式版 | RC 达到发布门槛 | 后续问题进入补丁版本或下一开发周期 | 对普通用户提供稳定兼容承诺 |

同一 Alpha 内可以包含多个独立任务，但实现必须拆分提交和回归结论；Alpha 编号表示可供集成验证的版本节点，不等同于每完成一个内部任务就发布一次。

### 当前执行窗口（`1.2.7` 发布后）

稳定版观察是持续维护通道，不是后续开发的前置条件。下载、匿名活跃、Issue 和 QQ 群反馈继续观察；只有出现可稳定复现且影响安装、启动、数据安全或大范围兼容性的高优先级回归时，才临时抢占当前开发任务。

| 工作流 | 当前状态 | 现在适合做的工作 | 与其他工作的关系 |
|--------|----------|------------------|------------------|
| `1.2.7` 稳定版维护 | 并行观察 | 汇总反馈、复现真实缺陷、只发布必要修复 | 不阻塞 `1.3.0` 开发 |
| Native 库名去品牌化 | 已完成 | 保持任务级中性别名、冲突检测、标准/API 19 双资源和真机回归 | 与认证缓存共同进入 `alpha.1`，但保持独立提交和回归结论 |
| Stub DEX 基线治理 | 静态治理完成 | 已建立指标守门、移除无消费日志，并统一 Application 关键字段写入的失败关闭策略 | 后续有限资源变体作为独立能力实施 |
| 桌面 Java 运行兼容 | 已完成 | 用户运行环境支持完整 JDK 8+，仅依赖 `java` 与 `keytool`；源码构建继续使用 JDK 17 | 内置 Java 17 免配置包仅作后续低优先级评估 |
| 认证缓存与资源能力 | `alpha.1` 候选完成 | 摘要、原子重建、协议握手、异常缓存自动回归及低版本运行回归已完成 | 发布 `alpha.1` 后收集兼容反馈，不阻塞后续 `alpha.2` 开发 |
| Root 策略与内存 DEX 实验 | 后续实施 | 接入兼容/严格策略与 GUI；内存路径保持默认关闭 | 共同进入 `alpha.2`，内存实验结果不阻塞后续 Beta |

用户诊断中的低风险静态预检可以按真实反馈独立插入，不必等待整条安全主线完成；但不得与高风险壳加载改动混在同一提交或候选版本中。

---

## 一、已知缺陷（Bug）

### `extractNativeLibs=false` 时新增壳库被压缩导致安装失败

| 项 | 内容 |
|----|------|
| **优先级** | 高 |
| **状态** | 已完成 |
| **涉及文件** | `crates/shield-core` Manifest 解析、ZIP 重写与端到端测试 |

**复现结果**：原 APK 显式设置 `android:extractNativeLibs="false"` 且本身不含 Native 库时，加固新增的四架构 `libmocikashield.so` 会被 apktool 重打包为压缩条目。产物虽通过现有 16 KB ZIP 偏移校验，仍在 Android 16 真机安装时报 `INSTALL_FAILED_INVALID_APK: Failed to extract native libraries`。

**修复方向**：保留原 Manifest 三态语义；仅在显式 `false` 时强制全部 `lib/**/*.so` 使用不压缩存储并按 16 KB 对齐，同时独立验证 Stub ELF `LOAD` 段。不得以无条件改成 `extractNativeLibs=true` 作为正式修复。

**完成结果**：核心已实现 Manifest 三态解析、按策略重写 Native 库和压缩条目复验；自动端到端测试覆盖原 APK 无 Native 库且显式为 `false` 的场景。Android 16 真机已确认该场景由安装失败修复为安装成功；已有 Native 库的 ARouter 样本已通过覆盖安装、冷启动和 Native 直接加载。API 23、API 35 16 KB 与 API 36 设备矩阵均已完成，加固产物保持 Manifest 语义，全部 `.so` 均为不压缩且 16 KB ZIP 对齐。

**完成条件**：有/无原生 Native 库的显式 `false` 样本均保持 Manifest 不变，所有 `.so` 为不压缩且 16 KB 对齐；完成自动结构测试、Android 16 真机安装启动，以及候选版本设备矩阵回归。详细设计见 [Android Native 库打包与加载兼容设计](../design/native-library-packaging.md)。

### Android 5.0～6.0 DEX 加载兼容

| 项 | 内容 |
|----|------|
| **优先级** | 高 |
| **状态** | 已完成 |
| **涉及文件** | `shield-stub`、运行时兼容测试、使用与设计文档 |

**目标**：在不创建第二个 ClassLoader、不改变 DEXB 格式的前提下，将运行时支持范围扩展到 Android 5.0（API 21）及以上。

**实施方案**：

1. API 24 及以上保持现有 JNI `addDexPath` 路径。
2. API 21～22 反射调用 `makeDexElements`，API 23 反射调用 `makePathElements`。
3. 所有版本均把解密 DEX Element 前插到原 `PathClassLoader`，保持唯一 defining loader。
4. 单元测试覆盖版本路由和 Element 顺序；API 21、23 设备覆盖首次启动、清除数据、多 DEX、ARouter 和 Native 库。
5. Android 4.4（API 19～20）使用独立工控兼容模式，不降低标准模式的 Native 构建基线。

**完成结果**：API 21 与 API 23 官方模拟器已完成单 DEX、双 DEX、真实 Application、首次安装、清除数据与覆盖安装回归；用户 Android 6.0 工控真机进一步确认首次启动、多 DEX、Native 库和主要硬件业务正常。标准模式最低支持范围已更新为 Android 5.0（API 21）及以上。

### Android 4.4 工控兼容模式

| 项 | 内容 |
|----|------|
| **优先级** | 高 |
| **状态** | 已完成 |
| **涉及文件** | `shield-stub`、Native 实验构建、运行时兼容测试、目标模式预检 |

**目标**：提供一个以 Android 4.4（API 19）为最低版本、可同时部署到 Android 4.4 与 Android 6.0 工控板的兼容加固产物，不降低现有标准模式的 NDK 与现代系统能力。

**已确定边界**：

1. 标准模式固定使用 NDK r29 `29.0.14206865` 和 API 21；工控兼容模式的 32 位 ABI 使用 NDK r25c `25.2.9519653` 和 API 19，64 位 ABI 继续使用 r29/API 21。
2. 支持 `armeabi-v7a`、`arm64-v8a`、`x86`、`x86_64`；暂不支持 `armeabi`、`mips` 和 `mips64`。
3. API 19～20 单独实现 Dalvik 加载路径，API 21 以上复用已验证的 ART 路径；DEXB v5 和签名校验协议不分叉。
4. 自动读取原 APK 的最低系统和 ABI 后推荐模式，由用户最终确认；包含 Native 库的 APK 只注入原有 ABI 对应的 Stub。
5. r25c 的 `armeabi-v7a` 产物要求 NEON；真实设备无 NEON 时再评估 r23c，不预先引入第三套正式工具链。

**完成结果**：现有 Rust Stub 使用隔离的 r25c、Rust 1.77.2 工具链构建 `armeabi-v7a/API 19` 产物，ELF 确认为 ARMv7/NEON，仅依赖 `libc.so` 与 `libdl.so`。同一个双 DEX 兼容加固 APK 已在 Android 4.4.2/API 19 `armeabi-v7a`、Android 5.0/API 21 `arm64-v8a` 和 Android 6.0/API 23 `arm64-v8a` 模拟器通过 Native 加载、对应 DEX 注入路径、自定义 Application、首次安装、清除数据与同签名覆盖安装回归；Android 6.0 工控真机完成详细业务回归，Issue #15 用户进一步确认 Android 4.4.2 `armeabi-v7a`/NEON 工控板能够正常运行。Android 4.4 用户未逐项覆盖全部业务测试，因此支持结论限定为该硬件组合的核心兼容验证通过，其他厂商设备继续按个案处理。GUI 已接入双资源选择，后端固定映射资源并校验 ABI。兼容工具链不得跟随标准构建升级。

**完成口径**：自动化与模拟器覆盖完整加载矩阵，Android 6.0 真机覆盖详细业务路径，Android 4.4.2 真机确认核心运行正常。未逐项完成的特定硬件交互不再阻塞当前兼容结论，后续发现问题按设备和场景单独处理。详细记录见 [Android 4.4 工控兼容设计](../design/android-4.4-compatibility.md)。

### V2/V3-only APK 预检与加固签名提取不一致

| 项 | 内容 |
|----|------|
| **优先级** | 高 |
| **状态** | 已完成 |
| **涉及文件** | `crates/shield-core/src/apk_inspect.rs`、`crates/shield-core/src/protect_api.rs`、`shield-stub/src/main/java/dev/mocika/shield/loader/Ld.java` |

**现象**：早期实现会把 V2/V3-only APK 误判为“未签名”。预检修复后，严格不含 `META-INF` 签名文件的 V2/V3-only APK 虽可通过预检，仍会在“处理 DEX”阶段报签名提取失败。

**根因**：预检和加固流程曾维护两套签名提取逻辑。预检已经使用 `apksigner` 识别 APK Signing Block，加固流程却仍然只检查 `META-INF/*.RSA|DSA|EC` 并调用 `keytool -jarfile`。后者不能可靠读取严格的 V2/V3-only APK，也可能读到已经失效的 V1 残留证书。

**最终修复方案**：

1. `check_apk()` 优先调用 `apksigner verify` 判断签名是否有效；只有工具不可用时才使用 APK Signing Block magic 和 V1 条目做预检降级。
2. `extract_apk_cert_fingerprint()` 统一调用 `apksigner verify --print-certs`，只接受验证成功的当前 APK 内容签名证书，不再把 `keytool -jarfile` 作为 APK 证书来源。
3. 加固流程直接复用 `extract_apk_cert_fingerprint()`，删除重复的 apktool 解包、临时 Java 编译和 V1 证书解析实现。
4. 指纹固定为当前 X.509 签名证书 DER 的 SHA-256、大写 64 位十六进制；忽略公钥摘要和 Source Stamp 证书。
5. DEXB v5 当前只保存一个指纹，因此宿主端和 Android 壳都明确拒绝多签名 APK，避免证书数组顺序不稳定导致运行时误判。
6. 加固后必须使用与输入 APK 当前证书相同的 keystore 签名；若改用其他证书，运行时按设计拒绝解密和启动。

---

### 大体积加固载荷导致状态预检漏报

| 项 | 内容 |
|----|------|
| **优先级** | 中 |
| **状态** | 已完成 |
| **涉及文件** | `crates/shield-core/src/apk_inspect.rs` |

**现象**：加固包可在真机正常解密运行，但 `check-apk` 返回 `already_protected: false`。

**根因**：MSHD 追加块布局为 `magic(4) + payload_len(4) + payload`，标记位于加密载荷起点。预检只扫描 `classes.dex` 最后 4 KB；当加密载荷超过 4 KB 时，标记必然落在扫描范围之外。

**最终修复方案**：

1. 流式扫描完整 `classes.dex`，每次只读取 64 KB，并保留 7 字节跨块重叠，不将完整 DEX 载入内存。
2. 只有 `MSHD` 后的 `payload_len` 恰好指向 `classes.dex` 文件末尾时才判定已加固，避免普通字节内容中的同名字符串造成误报。
3. 覆盖大于 4 KB 的载荷、跨读取分块的头部以及长度不一致的伪标记回归测试。
4. 核心 `protect_apk()` 在解包前再次执行加固状态检查，保证 GUI 预检漏报或 CLI 直接调用时也无法二次加固。

---

### 自动签名证书不一致仍生成无法启动的产物

| 项 | 内容 |
|----|------|
| **优先级** | 高 |
| **状态** | 已完成 |
| **涉及文件** | `crates/shield-core/src/protect_api.rs`、`apps/shield-gui/src-tauri/src/protect_runner.rs`、`apps/shield-gui/src/hooks/use-protect-workflow.ts` |

**现象**：原 APK 与默认自动签名证书不一致时，旧流程先完成加固，之后只提示“可能无法覆盖安装”，仍继续用不同证书签名。

**根因**：证书比较位于加固之后、签名之前，并且只作为前端警告。DEXB v5 已将原 APK 证书指纹绑定到密钥派生，不同证书签名后的实际结果是运行时无法解密和启动，而不只是覆盖安装失败。

**最终修复方案**：

1. 选择 APK 时立即比较原 APK 与默认自动签名证书，指纹不一致或读取失败都作为预检错误。
2. 前端只向 Tauri 后端传证书 ID；后端从本地证书库读取材料并提取指纹，不向前端返回密码。
3. `ProtectOptions` 接收计划输出证书指纹，核心在解包和创建输出前再次比较；任何错误均失败关闭。
4. 错误信息明确说明不同证书签名后的应用将无法启动，并要求选择原 APK 使用的证书。

---

### 匿名使用统计存在数据但页面始终为空

| 项 | 内容 |
|----|------|
| **优先级** | 中 |
| **状态** | 已完成 |
| **涉及文件** | `scripts/project_stats.py`、`apps/shield-gui/src-tauri/src/telemetry.rs` |

**现象**：Worker 已保存客户端启动、加固和签名数据，GitHub Pages 的应用使用指标仍显示为空，每日工作流却持续成功。

**根因**：页面脚本使用 Python 默认 `Python-urllib/*` 请求标识访问 Worker，被 Cloudflare 返回 `403`；异常被静默转换为空对象。另有 `sign_failed_count` 分支未累加，导致签名失败永远记为零。

**最终修复方案**：

1. 匿名统计请求设置项目专用 `User-Agent` 与 `Accept: application/json`。
2. 接口失败时输出任务警告并写入 `available: false`，页面明确显示不可用，不再伪装成“没有数据”。
3. 保持下载和仓库统计的失败隔离，匿名统计暂时不可用时仍生成页面。
4. 补齐签名失败计数，并覆盖请求头和失败状态单元测试。

---

### ARouter 运行期扫描在首次安装后路由失败

| 项 | 内容 |
|----|------|
| **优先级** | 高 |
| **状态** | 已完成 |
| **涉及文件** | `crates/shield-core/src/dex_packer/route_scanner.rs`、`shield-stub/src/main/java/dev/mocika/shield/loader/ARouterCompat.java`、`StubApp.java` |

**现象**：未启用 `arouter-register` 的 ARouter 1.5.1 应用，加固后首次安装或清除应用数据后路由跳转出现 `InterceptorService.doInterceptions(...)` 空指针；保留旧缓存的覆盖安装可能暂时正常。

**根因**：壳在真实 `Application.onCreate()` 返回后才补注册路由表，但 ARouter 已在 `ARouter.init()` 内查找并缓存空的 `InterceptorService`。事后补注册 Warehouse 无法回填该静态字段。

**最终修复方案**：

1. 在真实 Application 启动前读取加固阶段生成的路由清单，同步替换 ARouter 自有 `ROUTER_MAP` 缓存。
2. 同步当前版本名称与版本号，使 ARouter 跳过无法发现加密 DEX 的运行期扫描，并只加载当前路由表一次。
3. 扫描清单只保留 `Root`、`Providers`、`Interceptors` 三类入口，排除 `Group` 和内部类。
4. 可调试包会强制运行期扫描，因此继续提前注册当前路由表，保留首次安装和清除数据兼容能力。
5. 使用用户提供的多模块 Demo 真机验证原始基线、加固后首次安装和清除数据后三条路径，跨模块跳转及四类参数注入全部通过。
6. 使用跨加固方案升级样本验证旧缓存不会与当前路由表混合；新旧 APK 必须保持签名一致。

---

### 证书对比任务异常时 fail-open

| 项 | 内容 |
|----|------|
| **优先级** | 中 |
| **状态** | 已完成 |
| **涉及文件** | `apps/shield-gui/src-tauri/src/main.rs` → `compare_cert_fingerprints` |

**现象**：正常路径无问题；若 `spawn_blocking` 内部 panic（极少见），返回值为 `matches: true` + `error: Some(...)`，若前端只判断 `matches` 字段则会误判为"证书匹配"。

**根因**：
```rust
.unwrap_or_else(|e| CertCompareResult {
    matches: true,  // ← 异常时不应给默认值
    error: Some(...),
    ...
})
```

**最终修复方案**：将 `matches: true` 改为 `matches: false`（1行改动），异常时 fail-closed，前端已有 `error` 字段判断逻辑不受影响。

---

### 加固密钥未绑定签名指纹，IKM 固定可逆

| 项 | 内容 |
|----|------|
| **优先级** | 中 |
| **状态** | 已完成 |
| **涉及文件** | `crates/shield-core/src/protect_api.rs`、`crates/shield-core/src/dex_packer/packer.rs`、`shield-stub/src/main/rust/src/lib.rs`、`shield-stub/src/main/rust/src/crypto.rs`、`shield-stub/src/main/java/dev/mocika/shield/loader/Ld.java` |

**现象**：所有加固产物使用同一套根密钥材料（IKM），攻击者逆向 CLI 或 stub 拿到 `MocikaShield123!`，配合 DEXB 头部明文存储的 nonce，可完整重建解密密钥，解密任意加固 APK。

**根因**：加密链路为 `HKDF(ikm=DEFAULT_KEY, salt=nonce) → derived_key → ChaCha20-Poly1305`。nonce 每次随机（正确），但 IKM 固定为 `const DEFAULT_KEY: &str = "MocikaShield123!"`（`protect.rs:17`），stub 侧 `getDefaultKey()` 返回同一值作为解密 fallback。HKDF 在这里只做密钥拉伸，不提供保密性——IKM 固定等同于根密钥固定。

**注意**：nonce 随机化是正确的，解决了"相同明文产生相同密文"的问题，但不解决 IKM 被逆向后所有 APK 可被解密的问题。两者是不同层次的保护。

**最终修复方案**：将签名指纹绑定进密钥派生，在 Rust Native 层完成所有密钥运算，消除 Java 层硬编码。

**加固侧（CLI）改动**：

1. `protect.rs`：删除 `DEFAULT_KEY` 常量，随机生成 32 字节 IKM
2. `packer.rs`：`pack()` 函数签名增加 `ikm: &[u8]` 参数；DEXB 头部明文区新增 `ikm_len(1) + ikm[ikm_len]` 字段（紧跟 nonce 之前），供 stub 侧读取
3. HKDF info 字段由固定 `"mocika-shield-dex-key"` 改为传入的签名指纹字节，实现每个 APK 密钥与其证书绑定

**DEXB 头部明文区新布局（v5）**：
```
magic(4) + version(4) + dex_count(4) + sig_len(1) + signature[sig_len]
+ ikm_len(1) + ikm[ikm_len] + nonce(12) → 密文
```

**运行时侧（stub Rust Native）改动**：

1. `bin_loader.rs`（解析层）：按新格式解析 IKM 字段
2. `crypto.rs`：`derive_key` 签名改为 `derive_key(ikm: &[u8], nonce: &[u8;12], cert_fp: &[u8]) -> [u8;32]`，info 字段传入证书指纹
3. `lib.rs`：`extractAndDecryptFromDex` 中，从 payload 取出 IKM 后，调 `jni_get_actual_signature` 获取当前证书指纹，将两者传入新 `derive_key`；删除 `getDefaultKey()` JNI 函数
4. `Ld.java`：删除 `getDefaultKey()` native 声明与 `getKey()` 方法（不再需要）；`extractDexFiles` 中调用路径直接传 `ctx` 即可（签名在 Native 层内部获取）

**前置条件**：已签名 APK 才能加固（未签名无法获取指纹，签名提取失败直接报错退出，不降级）。

**向后兼容**：v5 格式与 v4 不兼容（头部新增 IKM 字段），旧加固 APK 需重新加固。

---

## 二、演进功能（新增）

### 桌面端任务体验与显式签名选择

| 项 | 内容 |
|----|------|
| **优先级** | 高 |
| **状态** | 已完成 |
| **涉及模块** | `apps/shield-gui` 前后端、任务事件协议、测试与文档 |

**目标**：让加固和签名任务在页面切换后保持状态，提供真实步骤、耗时和失败诊断，并允许用户在每次加固时明确决定是否签名以及选择证书。

**实施约束**：

- 每个任务使用独立任务编号，任务事件必须携带任务类型、状态、步骤、时间和日志，禁止加固与签名事件串扰
- `shield-core` 只负责核心流程和结构化进度，不依赖 Tauri；Tauri 后端负责任务生命周期和前端事件映射
- 加固后的签名与中间产物清理由同一个后端任务完成，避免前端卸载导致流程中断
- 加固页提供“加固后签名”开关和证书选择，本次选择不修改默认证书；首次默认值兼容证书原有自动签名偏好
- 侧边栏显示后台任务运行状态；任务日志最多保留有限条数，且不得包含证书密码
- 后续批量加固和任务历史应复用任务快照，不重新建立第二套状态协议

**验收范围**：页面切换恢复、重复任务拦截、证书删除后的选择回退、加固与签名日志隔离、取消后不再签名、失败步骤复制、CLI 行为兼容。

### 独立证书管理页与本地证书数据库

| 项 | 内容 |
|----|------|
| **优先级** | 高 |
| **状态** | 已完成 |
| **涉及模块** | `apps/shield-gui` 前后端、文档 |

**目标**：提供独立证书管理能力，支持多套证书、默认证书、自动签名、创建/导入/校验/删除，并将结构化数据保存到本地 SQLite 数据库。

**范围**：

- 新增“证书”页面，采用桌面风格的单列表管理视图，导入、创建、编辑通过弹框完成
- 设置页只保留应用级配置，不再长期承载完整签名表单
- 证书数据、默认证书、签名密码改为本地 `shield.db` 管理
- 应用内新建证书默认放到应用数据目录 `keystores/`
- 导入已有证书默认仅记录原始路径，可选“导入并托管”
- 已保存证书的材料不可通过编辑入口修改；编辑只维护显示名称、备注、签名版本和自动签名偏好

**产品约束**：

- 本工具为本地离线软件，允许本地持久化保存 `keystore_password` 与 `key_password`
- 默认不依赖系统 Keychain
- 日志、错误信息和调试输出不得包含密码明文
- 如需更换 `keystore` 文件、类型、Alias 或密码，应新增证书记录，不复用旧记录静默替换
- 加固页、签名页最终都只消费证书管理页维护的数据

**已落地能力**：

- 已统一 `config.toml` / `shield.db` / `keystores/` 的职责
- 已引入 `shield.db`，保存证书列表、默认项、签名密码与校验状态
- 已支持导入、创建、偏好编辑、校验、设默认、删除
- 已集成 `keytool -genkeypair`，新建证书默认托管到 `keystores/`
- 加固页已消费默认证书与自动签名配置
- 签名页已支持选择证书
- 设置页已移除长期签名表单，只保留应用级配置

### 版本更新提示

| 项 | 内容 |
|----|------|
| **优先级** | 高 |
| **状态** | 已完成 |
| **涉及模块** | shield-gui、发布脚本 |

**方案**：不使用 `tauri-plugin-updater`，改为轻量方案——启动时请求 GitHub Releases API 对比版本号，有新版本时提示用户，点击后打开浏览器跳转 Release 页面，由用户自行下载。

**原因**：`tauri-plugin-updater` 各平台安装行为差异大——deb 需要 root + 密码弹窗，dmg 不被 updater 支持，Windows 需要 UAC 提权。让用户自己下载安装包更可控，实现也更简单。

#### 发布仓库

GitHub 仓库 `mocikadev/mocika-shield`，源码、介绍文档和 Release 包均在同一仓库维护。

Release 页面 URL：`https://github.com/mocikadev/mocika-shield/releases`

#### 版本检查逻辑（GUI）

**GitHub API**：`GET https://api.github.com/repos/mocikadev/mocika-shield/releases/latest`

- 无需认证，公开仓库限流 60次/小时，配合 24h 缓存完全够用
- 仅使用两个字段：`tag_name`（版本号，如 `"v1.2.0"`）和 `html_url`（Release 页跳转链接）
- 返回 **404**（仓库无任何 Release）时静默忽略，不展示任何提示
- `latest` 排序依据是提交时间而非语义版本，因此我们自行做语义版本比对，不依赖 GitHub 的顺序

```
check_update command（后端）：
  接收参数 force: bool（true = 跳过缓存，强制重新请求）
  → 若 force = false：读 store 中 update_last_check 时间戳
                      距上次检查 < 24h → 直接返回缓存的 update_last_result
  → 否则：GET .../releases/latest
          超时 5 秒
          404 或失败 → 返回 Err(错误信息)
          成功 → 解析 tag_name：
                  · strip 开头的 v/V 前缀（大小写不敏感）：tag_name.trim_start_matches(|c: char| c == 'v' || c == 'V')
                  · CARGO_PKG_VERSION 编译期注入，格式为 "x.y.z"（无前缀）
                  · 解析失败（格式不合法）→ 静默忽略，不展示任何提示
                  · 解析成功 → 做语义版本比对
              → 结果写入 store（update_last_check + update_last_result）
              → 返回 Ok(UpdateCheckResult)
```

**调用方式**：
- 启动时自动检查：`check_update(force: false)`，走缓存逻辑
- 关于页手动点击：`check_update(force: true)`，强制跳过缓存重新请求，行为符合用户直觉

**`check_update` 返回结构**：

```rust
struct UpdateCheckResult {
    has_update: bool,
    latest_version: Option<String>,  // "1.2.3"
    update_level: Option<String>,    // "patch" | "minor" | "major"
}
```

**版本比对逻辑**：按最高位差异决定级别，从 major → minor → patch 依次比较，第一个不同的位即为级别：

```
remote(2.0.0) vs local(1.9.9) → major 不同 → "major"
remote(1.2.0) vs local(1.1.5) → minor 不同 → "minor"
remote(1.1.2) vs local(1.1.1) → patch 不同 → "patch"
```

**启动时自动检查**（前端调用 `check_update`，忽略错误分支）：

```
有新版本，检查 store 中 dismissed_version：
  → dismissed_version == latest_version 且 update_level != "major" → 跳过，不展示
  → 否则按 update_level 分级展示：
      · patch / minor → 顶部提示条（导航栏下方，推开内容区）
          patch：可一键关闭，关闭时写入 dismissed_version
          minor：持续显示直到手动关闭，关闭时写入 dismissed_version
      · major → 启动时弹窗，每次启动都弹，直到用户更新为止（不受 dismissed_version 控制）；
               延迟 1~2 秒弹出，避免主界面未渲染完成时打断用户操作
用户点击"前往下载" → tauri::api::shell::open() 打开 Release 页面
```

**store 字段**：

| key | 说明 |
|-----|------|
| `update_last_check` | 上次检查时间戳（Unix 秒） |
| `update_latest_tag` | GitHub 返回的原始 latest tag（如 `"1.0.1"`），每次读缓存时重新 compare_semver 计算结论 |
| `update_release_url` | Release 页面 URL |
| `dismissed_version` | 用户已关闭提示的版本号，major 升级不受此控制 |

**涉及改动**：
- `apps/shield-gui/src-tauri/src/main.rs`：新增 `check_update` Tauri command，含缓存逻辑
- `apps/shield-gui/src/App.tsx`：顶部提示条、关于页检查更新入口与结果展示
- `apps/shield-gui/src-tauri/Cargo.toml`：新增 `reqwest`（`rustls-tls` feature）、`tauri-plugin-shell`
- `apps/shield-gui/src/lib/i18n.ts`：新增更新相关文案 key

**`reqwest` 请求注意事项**：
- 必须设置 `User-Agent` header，否则 GitHub API 返回 403：`User-Agent: mocika-shield/{CARGO_PKG_VERSION}`
- 使用 `rustls-tls` feature，不依赖系统 SSL，三平台行为一致

**打开浏览器**：Tauri v2 的 `shell::open()` 已从核心移到 `tauri-plugin-shell`，需在 `Cargo.toml` 中添加该插件并在 `main()` 中注册。

#### 关于页"检查更新"交互

点击按钮后三种状态：
- **检查中**：按钮显示 loading
- **有新版本**：显示远端版本号 + 跳转 Release 页按钮（措辞按 patch/minor/major 分级）
- **已是最新**：按钮旁提示"已是最新版本"
- **失败**：按钮旁提示"检查失败，请确认网络连接"（仅手动触发时展示，自动检查静默忽略）

#### i18n 新增文案

| key | 中文 | 英文 |
|-----|------|------|
| `checkingUpdate` | 检查中 | Checking |
| `upToDate` | 已是最新版本 | You're up to date |
| `updateAvailable` | 发现新版本 | New version available |
| `majorUpdate` | 重大版本更新 | Major Update |
| `updateFailed` | 检查失败，请确认网络连接 | Check failed, please verify your network |
| `viewRelease` | 查看更新详情 | View Release |
| `ignore` | 忽略 | Ignore |

#### 发布流程（GitHub Actions 自动上传同一 Release）

推送 `vX.Y.Z` tag 后，由 `.github/workflows/release.yml` 并行构建 Linux、macOS、Windows 产物，最后创建或更新同一个 GitHub Release；稳定版本保持 Draft，预发布版本直接标记为 Pre-release。

- Linux Tauri、macOS Tauri、Windows 各自上传 workflow artifact
- `publish` job 汇总所有 artifact，上传到 `vX.Y.Z` Release
- Release Notes 与正式发布仍由维护者最终确认

详细规则见 [docs/process/release.md → GitHub Actions CI/CD](release.md#github-actions-cicd)。

**发布脚本**：各平台脚本（`release-linux.sh` 等）仍可本地运行，用于复现或排查 CI 发布问题。

#### 发布仓库内容（README.md）

中文，包含：
- 功能介绍：DEX 加密保护、防重打包（证书绑定）、多架构支持（arm64 / armeabi-v7a / x86 / x86_64）、桌面 GUI 拖拽操作、内置签名工具
- GUI 主界面截图一张
- 下载链接（指向 Releases 页）
- 简单使用说明

不包含：加密算法细节、低特征实现原理、更新日志（用 GitHub Release Notes 代替）。

---

### 反调试检测

| 项 | 内容 |
|----|------|
| **优先级** | 高 |
| **状态** | 已完成 |
| **涉及模块** | shield-stub（Android 端） |
| **方案** | 运行时多维度检测，检测到立即拒绝启动 |

**已实现**（`shield-stub/src/main/rust/src/anti_debug.rs`）：

- `check_tracer_pid()`：读 `/proc/self/status` 中 `TracerPid` 字段，非零即拒绝（检测 adb / IDA / lldb 等 ptrace 附加）
- `check_frida_maps()`：扫描 `/proc/self/maps` 中的 Frida 库特征字符串（`frida-agent` / `frida-gadget` / `libfrida` / `gum-js-loop`）
- `check_frida_threads()`：遍历 `/proc/self/task/*/comm`，匹配 Frida 依赖的 GLib 线程名（`gmain` / `gdbus` / `pool-frida`），覆盖 phantom-frida 重命名库文件的场景

检测逻辑位于 Rust Native 层。`RuntimeSecurity` 在每次进程启动、读取 DEX 缓存前调用独立检查入口；`f2` 解密入口继续执行同一检查作为纵深保护。命中后只向 Java 返回通用失败，不透露具体规则。无额外 Cargo 依赖（纯 `std::fs`）。

**后续安全演进**：每次进程启动现在都会在读取缓存前执行 Native 环境检查，解密入口同时保留纵深检查；缓存本身仍缺少签名锚定的完整性验证。解密后的完整业务 DEX 会保存在应用私有目录，Root、可调试包 `run-as`、注入或已攻破进程环境可能提取明文。缓存完整性不能解决读取风险，后续按以下顺序分别推进：

1. `1.2.7` 已完成每次启动安全检查、三平台发布构建和设备矩阵验证，后续反馈转入并行维护通道。
2. 分别实现 Native 库名去品牌化以及认证缓存、原子重建、资源能力协议；两项完成集成验证后发布 `1.3.0-alpha.1`，实现过程保持独立提交和回归结论。
3. 在 `1.3.0-alpha.2` 接入兼容/严格 Root 环境策略和 GUI 任务级选项，并加入默认关闭的 API 29 以上内存 DEX 实验；实验结果不阻塞后续阶段。
4. 在进入 `1.3.0-beta.1` 前完成 Stub DEX 指标基线和低风险静态清理；Beta 起冻结功能与协议，只做真实 APK、设备矩阵和性能回归。
5. `1.3.0-rc.1` 冻结发布产物，只修复阻塞正式发布的问题，随后发布 `1.3.0`；内存 DEX 不作为该正式版承诺能力。
6. 从 `1.4.0-alpha.1` 开始把 API 29 以上内存 DEX 作为生产候选，经 `1.4.0-beta.1` 扩大验证和 `1.4.0-rc.1` 发布冻结后，再决定发布 `1.4.0` 正式版。

完整边界、兼容策略、任务和验收要求见 [Android 运行时安全与 DEX 缓存演进设计](../design/runtime-security.md)。

---

### DEX 结构与方法代码分离研究

| 项 | 内容 |
|----|------|
| **优先级** | 中 |
| **状态** | 待调研 |
| **涉及模块** | `shield-core` DEX 转换与载荷协议、Stub Native 恢复、`DexInjector` |

**方向**：完整 DEX 加密只能阻止静态直接提取；运行时形成完整明文后仍可能被分析。后续研究将 DEX 元数据与方法 `code_item` 分离，结构载体和代码载荷分别认证加密，并优先在内存中重建。原型成功后再评估分片恢复，不把单纯破坏 DEX header 作为有效保护。

**前置条件**：`1.3.0` 运行时安全主线稳定，且 `1.4.0` API 29 以上完整 DEX 内存加载达到正式生产门槛。首个原型只覆盖 API 29 以上、单 DEX 和普通 Java 方法，不进入 GUI 或正式资源。

**进入下一阶段的条件**：结构载体不能通过机械修复直接恢复完整业务代码；功能、性能和提取实验能够证明相对完整内存 DEX 路径存在可量化收益。详细层级、模块边界、兼容门槛和停止条件见 [DEX 结构与方法代码分离研究](../design/dex-code-separation.md)。

---

### CLI 能力补全

| 项 | 内容 |
|----|------|
| **优先级** | 高 |
| **状态** | 待实现 |
| **涉及模块** | shield-cli |

**说明**：

当前 CLI 只有单一隐式命令，计划改造为双子命令结构：

```
shield protect -i input.apk -o output.apk [--apktool <path>] [--resources <path>] [--keep-tmp]
shield sign    -i input.apk -o output.apk --ks keystore.jks --ks-pass <pass> --key-alias <alias>
```

同时支持 `--config <path>` 从文件读取参数，命令行优先级高于配置文件，方便 CI 复用。CLI 的人工配置必须与 GUI 自动维护的 `config.toml` 明确区分。

---

### GUI 批量加固

| 项 | 内容 |
|----|------|
| **优先级** | 中 |
| **状态** | 待实现 |
| **涉及模块** | shield-gui |

**说明**：

- 支持拖拽多文件或多选文件选择器
- 队列列表展示每个 APK 的状态（等待 / 加固中 / 完成 / 失败）
- 每个 APK 独立进度条，失败不中断后续任务
- 输出路径规则与单文件加固一致（`{name}_protected.apk`）

---

### GUI 交互反馈补全

| 项 | 内容 |
|----|------|
| **优先级** | 中 |
| **状态** | 已完成 |
| **涉及模块** | shield-gui |

**已实现**：

- 拖拽非 APK 文件时在文件选择区下方展示明确错误提示
- 预检与后台处理期间增加 loading 状态指示
- 错误信息支持一键复制，方便用户反馈问题

---

### 版本号统一管理

| 项 | 内容 |
|----|------|
| **优先级** | 中 |
| **状态** | 已完成 |
| **涉及模块** | 构建系统 |

**已实现**：

- 以 `scripts/bump-version.sh` 统一同步 `shield-core`、CLI、stub、GUI 的版本号，避免多 crate / 多端版本漂移
- 新增 `scripts/bump-version.sh`，一条命令统一更新所有 Cargo.toml 和 `tauri.conf.json` 中的版本号，避免手工同步漏改

---

### stub Java 层混淆（R8）

| 项 | 内容 |
|----|------|
| **优先级** | 低 |
| **状态** | 已否决（已由壳 Java 类名与 SO 字符串联动混淆覆盖） |
| **涉及模块** | shield-stub |

**说明**：

壳 Java 类名与 SO 字符串联动混淆在构建期实现了类名/方法名的完整混淆，并将混淆结果同步到 Rust `.so`，已覆盖本条目的目标。单独立项意义不大，关闭。

---

### 测试覆盖补全

| 项 | 内容 |
|----|------|
| **优先级** | 低 |
| **状态** | 进行中 |
| **涉及模块** | shield-cli、shield-stub |

**已完成**：

- `protect` 命令：XML Manifest 解析、Adler32 校验、ABI 检测、DEX 处理相关单元测试
- `sign` 命令：KeystoreType 自动识别、alias 解析单元测试
- 自有最小 Android 测试夹具覆盖“编译 → 原始签名 → 加固 → 再签名 → 产物结构与签名验证”的无设备端到端链路

**仍缺失**：

- Android 模拟器自动安装、首次启动与清除数据后的运行时回归
- JNI 降级路径测试（native 失败时 Java 反射是否正确接管）

---

### 关于页构建信息

| 项 | 内容 |
|----|------|
| **优先级** | 低 |
| **状态** | 已完成 |
| **涉及模块** | shield-gui |

**已实现**：

- 后端 `apps/shield-gui/src-tauri/build.rs` 与前端 `apps/shield-gui/build.rs` 在编译期分别注入 `GIT_HASH`、`BUILD_DATE`
- 关于页展示版本号、git commit hash（8位）、构建日期，以及运行时检测到的 apktool / apksigner 版本
- 手动检查更新按钮（复用版本更新提示的 `check_update` command）

---

### 发布包文件名加入系统标识

| 项 | 内容 |
|----|------|
| **优先级** | 低 |
| **状态** | 已完成 |
| **涉及模块** | scripts/release-linux.sh、scripts/release-macos.sh、scripts/release-windows.ps1 |

**已实现**：各平台发布脚本统一将 Tauri 构建产物重命名为含版本号与平台标识的格式：

```
MocikaShield_{VERSION}_linux_amd64.AppImage
MocikaShield_{VERSION}_linux_amd64.deb
MocikaShield_{VERSION}_macos_universal.dmg
MocikaShield_{VERSION}_macos_aarch64.dmg
MocikaShield_{VERSION}_windows_x64_setup.exe
```

---

### 降低壳特征识别难度

| 项 | 内容 |
|----|------|
| **优先级** | 高 |
| **状态** | 已完成 |
| **涉及模块** | shield-stub、scripts、shield-cli |

**背景**：对加固后 APK 进行静态分析，发现三处特征过于明显，攻击者无需破解加密即可精准定位壳代码入口。DEX 加密本体有效（还原难度 ⭐⭐⭐），但识别和 hook 入口过于容易。

#### 移除 SO 库源码路径字符串（已完成）

**现象**：`libmocikashield.so` 中可见 `shield-stub/src/main/rust/src/bin_loader.rs`（Rust panic location 残留）。

**修法**：将生产代码中所有 `unwrap()` 替换为 `map_err(|_| "静态字符串")?`，彻底消除 `#[track_caller]` 将文件路径写入 `.rodata` 的问题。

#### JNI 改为动态注册，隐藏函数名（已完成）

**现象**：`Java_dev_mocika_shield_loader_BinLoader_decryptAndDecompress` 等核心 JNI 函数名完全可读，可被 Frida 精准 hook。

**修法**：在 `JNI_OnLoad` 中用 `RegisterNatives` 手动注册，Rust 侧函数改为私有函数（无 `#[no_mangle]`），`.dynsym` 中只剩标准的 `JNI_OnLoad`。

#### 壳 Java 类名与 SO 字符串联动混淆（已完成）

**现象**：`dev/mocika/shield/loader/BinLoader`、`StubApp` 等类名在 DEX 和 `.so` 的 `.rodata` 中均为明文，一眼识别出壳来源。

**已实现方案**：

1. **构建期联动**：Gradle 完整构建（含 R8）→ 解析 `mapping.txt` → 混淆类名/方法名注入 Rust 编译期常量（`env!` 宏）→ 重新编译 `.so`，DEX 与 `.so` 字符串保持一致
2. **BinLoader 重命名为 Ld**：R8 内置 `keepclasseswithmembernames` 会锁住含 native 方法类的名称，直接在源码层将类重命名为无意义短名，规避规则，同时消除 `BinLoader` 字符串
3. **TAG 常量去特征**：`MocikaBinLoader/MocikaStubApp/MocikaARouterCompat` → `lx/ax/rx`；R8 将 TAG 常量完全内联消除，DEX 中不存在这些字面量
4. **错误消息去特征**：`FindClass BinLoader:`、`getSignatureSha256`、`mocika:` 等前缀均替换为短码（`e1/e2/e3/e4`）；品牌字符串 `Mocika Shield` 从旧版兼容错误消息中移除
5. **正常流程日志删除**：`Log.i / Log.d` 全部删除，仅保留 `Log.w / Log.e`，切断 logcat 暴露解密→注入→替换 Application 行为链路

**最终产物静态分析结果（arm64 .so）**：

| 特征 | 结果 |
|------|------|
| `BinLoader` | ✅ 完全消除 |
| `MocikaStubApp` / `MocikaARouterCompat` | ✅ 完全消除 |
| `mocika:` / `Mocika Shield` | ✅ 完全消除 |
| 动态导出符号 | ✅ 仅 `JNI_OnLoad` |
| 绝对源码路径 | ✅ 完全消除 |
| Application 类名（Manifest）| `msk.b`（R8 混淆后） |
| 壳 Loader 类名（DEX）| `dev.mocika.shield.loader.Ld` |
| native 库名 | 已按加固任务生成中性别名，并联动等长改写 Stub DEX；资源包内部仍使用规范名称 |

#### Native 库名去品牌化与按任务别名（已完成）

固定 `libmocikashield.so` 会直接暴露工具身份；改成另一个固定名称只会形成新指纹，纯十六进制随机名称又过于突兀。当前已采用固定长度的中性自然别名：每次加固生成无冲突名称，联动等长修改 Stub DEX 的加载占位符，并把四 ABI 规范库映射到同一个任务别名。

该能力只移除品牌和固定文件名特征，不宣称隐藏加固行为；不模仿用户业务库、不覆盖已有 `.so`，不增加 GUI 配置。实现必须保持 API 19、`extractNativeLibs=false`、16 KB ZIP 对齐、ARouter 和覆盖安装兼容。完整协议、失败策略和验收矩阵见 [Native 库名称去品牌化与按任务别名设计](../design/native-library-alias.md)。

#### Stub DEX 最小化与分阶段加载（待实现/待实验）

当前完整 Stub DEX 同时包含基础启动、ARouter、Android 9 共享库和低版本注入逻辑。后续先建立 DEX 类/方法/字符串指标，清理合成类与无消费入口的诊断信息，再根据运行基线、ARouter 和系统共享库三个有限维度选择预构建变体，减少默认产物不需要的静态暴露面。

把更多壳逻辑加密保存并在运行时加载只保留为 API 29 以上独立实验：初始 DEX 仍保留最小 Application 引导，二阶段代码不得成为业务类的 defining loader。每个 ABI 的 SO 重复内嵌密文不作为默认方案，运行期动态生成 DEX 明确不采用。主路线可以独立排期；二阶段实验必须等待高版本内存 DEX 路径稳定，不阻塞正式版本。详细方案与停止条件见 [Stub DEX 最小化与分阶段加载可选演进](../design/stub-dex-minimization.md)。

---

## 三、已否决

| 功能 | 原因 |
|------|------|
| 签名密钥系统级存储（Keychain 等） | 每次使用需认证，体验差 |
| resources.zip 加密 | 密钥必须内置工具中，安全收益极低 |
| 加固历史记录 | 实际意义不大 |

---

*最后更新：2026-07-29*

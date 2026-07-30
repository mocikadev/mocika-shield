# 回归测试清单

本文档记录发布前和关键改动后的手动回归检查项。自动化测试不能覆盖桌面安装包、系统 Java 环境、真实 APK 安装启动等场景时，按本清单补足验证。

## 自动端到端加固回归

修改加固、签名、Manifest、DEXB、壳资源或 ZIP 对齐链路后执行：

```bash
make build-stub
bash tests/scripts/run-protect-e2e.sh
```

脚本使用项目自有最小 Android 测试夹具，验证从源码 APK 到已签名加固产物的完整无设备链路。手动触发完整 CI 时，“Android 壳构建”任务也会执行该测试。

连接测试设备后执行以下命令，分别启动未加固双 DEX 基线，并以同一签名覆盖安装加固包。加固包需连续启动两次，覆盖首次解密和缓存命中路径；每次都必须观察到 `Application`、`Activity` 和 `classes2.dex` 中 `SecondaryMarker` 的成功标记：

```bash
RUN_DEVICE_TEST=1 bash tests/scripts/run-protect-e2e.sh
```

## 使用时机

- 发布 `rc` 或稳定版本前
- 修改加固、签名、证书管理、Java 环境检测、APK 对齐逻辑后
- 修改 Tauri 打包配置、内置资源、发布脚本后
- 修复用户反馈的安装、签名、加固失败问题后

## 基础环境

| 项 | 检查点 |
|----|--------|
| Java | 已安装 JDK 8+，`java`、`keytool` 可执行 |
| Android 工具 | 发布包内置 `apktool.jar`、`apksigner.jar`、`resources.zip` |
| 测试 APK | 使用自己拥有合法权利、可安装启动的已签名 APK |
| 测试证书 | 使用测试 keystore / p12，不使用生产证书 |
| 发布包 | 优先使用 GitHub Release 下载的安装包验证，不只验证本地裸二进制 |

Java 兼容下限调整或内置 JAR 升级后，必须使用真实 JDK 8 完成 `apktool` 解包/回编、PKCS12 创建、APK 签名及 v1/v2/v3 校验；源码和 Android 壳构建仍使用 JDK 17。

## GUI 基础检查

- 应用能正常启动，不出现白屏或资源缺失
- 关于页显示版本号、构建 hash、构建日期
- 关于页能显示 Java 环境状态，点击“重新检测环境”后状态更新
- 设置页能切换主题和语言，重启后配置仍然保留
- 侧边栏展开、折叠、页面切换无明显布局跳动

## 证书管理

- 导入已有 `jks` / `p12` 证书成功
- 导入证书时错误密码会失败，并显示可理解的错误信息
- 创建新证书成功，默认类型为 PKCS12
- 创建证书时 Keystore 密码不足 6 位会明确提示
- Key 密码留空时使用 Keystore 密码；填写时不足 6 位会明确提示
- PKCS12 Alias 使用大写输入时可校验通过，并保存 keystore 实际返回的 Alias
- 证书可设为默认，重启后默认项仍然正确
- 删除证书后，签名页和加固页不会继续使用已删除证书
- 证书列表不展示密码明文，日志和错误信息不包含密码明文

## 签名流程

- 签名页选择 APK 后能识别文件名、大小和签名状态
- 加固页和签名页在任务开始前均可修改输出路径，任务开始后输出路径保持固化
- 选择证书后可完成签名，生成 `{name}_signed.apk`
- 签名输出会自动清理同名 `.idsig`
- 签名成功后主操作区只保留“继续签名”入口
- 签名开始后 APK、证书和输出路径保持固化，完成或失败后仍不可修改；点击“继续签名”后恢复可编辑状态
- 使用错误证书或错误密码时能给出明确失败提示
- 签名后的 APK 可以安装或通过 `apksigner verify` 校验
- 签名期间切换到其他页面再返回，步骤、耗时和日志保持完整
- 加固任务与独立签名任务只接收各自任务编号的日志

## 加固流程

- 加固页选择 APK 后能完成预检
- 非 APK 文件会立即提示错误，不进入加固流程
- 未签名 APK 会被阻止或明确提示
- 无 `META-INF` 签名文件的严格 V2/V3-only APK 能通过预检并完成签名指纹提取
- 多签名 APK 会明确提示 DEXB v5 仅支持单签名，不生成可能无法启动的产物
- 已加固 APK 在 GUI 预检和核心加固入口都会被拒绝，不创建二次加固产物
- 默认自动签名证书与原 APK 指纹不一致或无法读取时，预检直接失败，不生成加固产物
- 加固成功后生成 `{name}_protected.apk`
- 开启自动签名并存在默认证书时，生成 `{name}_protected_signed.apk`
- 加固页可关闭签名，也可为本次任务选择非默认证书；选择不会修改全局默认项
- 任务开始后锁定 APK、签名开关和证书选择；完成、失败或取消后保持本次配置，只能通过“继续加固”开始新任务
- 任务执行或结果展示期间，证书列表变化不会改写本次输出路径、步骤列表和进度状态
- 选择已被删除的证书后，页面自动回退到有效证书或提示没有可用证书
- 加固期间切换页面后任务继续执行，侧边栏显示运行状态，返回后进度不丢失
- 同一类型任务执行期间再次启动会被明确拒绝
- 取消加固后不会继续执行签名和中间产物清理之外的后续步骤
- 日志至少覆盖解包、修改 Manifest、提取签名、加密 DEX、注入壳资源、重新打包、对齐、签名、完成等关键步骤
- 正常进度界面不重复展示任务日志；失败错误支持复制，内部任务日志不包含密码明文
- 加固失败时错误信息可复制，且不包含密码明文
- 输入 APK 与自动签名输出的 `certificate SHA-256 digest` 一致
- 加固后的 APK 可安装并正常启动

## Android 运行时兼容

- API 21、22 使用 `makeDexElements` 路径，真实 Application 与多 DEX 类均成功加载
- API 23 使用 `makePathElements` 路径，真实 Application 与多 DEX 类均成功加载
- API 24 及以上继续使用 `addDexPath` 路径，真实 Application 与多 DEX 类均成功加载
- API 21、23 分别验证单 DEX、多 DEX、真实 Application、ARouter 和 Native 库加载
- 同一测试 APK 在 API 21、23、24 设备上完成冷启动、清除数据后首次启动和覆盖安装启动
- API 19～20 仅通过“Android 4.4 工控兼容”模式提供；不得将 API 21 构建的 Native 库作为兼容产物交付，其他 ABI、非 NEON 或未知厂商设备不得沿用现有真机结论直接宣称已验证
- API 19 实验 Native 库必须由 r25c、Rust 1.77.2、`--platform 19` 构建，并通过 `scripts/verify-android-api19-native.sh` 的架构、依赖、动态符号与 ELF 审计
- API 19 Native 依赖升级后必须执行 `tests/scripts/run-api19-native-probe.sh`，确认生产库能够进入 `JNI_OnLoad`，不能只以链接成功作为兼容结论
- 工控兼容产物必须使用同一个 APK 在 API 19 和 API 23 设备验证，不能分别生成两份业务 APK 代替跨版本回归
- API 28 对声明 `uses-library` 且业务 DEX 含同名编译桩的 APK，必须确认系统共享库类优先解析；Issue #17 Apache HTTP 样本不得出现 `RuntimeException: Stub!`

## 运行时安全与 DEX 缓存

- 首次启动与缓存命中的二次启动都必须执行环境安全检查
- 解密入口保留纵深检查，不能只依赖 `StubApp` 单一调用点
- 缓存文件数量、名称、路径、只读状态、大小和根摘要必须全部匹配后才能加载
- 缓存缺失、多余、损坏或权限异常时必须整体失效并重新解密；无法安全清理时不得继续加载
- 相同 `versionCode` 的不同 APK 覆盖安装后不得复用旧业务 DEX
- DEX 文件打开后立即设为只读，再通过已打开的文件描述符写入
- Root 兼容策略不得阻止普通启动；严格策略只根据高置信度信号阻止解密
- 缓存摘要只验证来源和完整性，不得把通过认证的明文缓存视为不可读取
- 不可调试正式包通过普通 ADB 访问私有目录应失败；可调试包需验证并记录 `run-as` 提取边界
- Root 文件管理器、ADB Root 和进程注入场景需分别记录首次、二次及重启后的完整业务 DEX 暴露情况
- Android 4.4 工控兼容任务默认使用兼容策略，不能因弱信号误杀
- 标准资源与 Android 4.4 资源的 `runtime_protocol`、`cache_schema` 和策略能力必须一致
- 内存 DEX 候选必须单独验证多 DEX、ARouter、API 28 共享库、Native 库、GC 后类加载、16 KB、冷启动耗时与峰值内存
- split 验证必须区分“安装后可运行”与“每个代码 split 已加密”，不得把明文动态特性可加载视为保护通过
- Instrumentation 必须使用与目标 APK 同签名的独立测试 APK，验证目标 Context 持有代理且业务类没有泄漏到测试加载器
- 文件/内存迁移必须保持签名和应用数据，双向覆盖后分别启动；崩溃自动回退需另验认证状态与独立进程重启
- 内存路径启动前必须同步持久化待确认状态；遗留内存待确认状态只能在下一进程回退到文件路径，不得在同一进程切换加载器
- 文件回退连续失败、认证状态被篡改、包装密钥缺失或记录不完整时必须失败关闭；新载荷身份不得继承旧载荷的回退状态

## APK 对齐与 Google Play 兼容

- 加固输出 APK 已执行 ZIP 对齐
- 普通条目按 4 字节对齐
- `lib/**/*.so` 按 16 KB 对齐
- 原 APK 显式设置 `extractNativeLibs=false` 时，加固后保持该值不变，全部 `lib/**/*.so` 必须使用 ZIP `Stored` 不压缩存储
- 显式 `extractNativeLibs=false` 的回归必须同时覆盖原 APK 已有 Native 库和原 APK 不含 Native 库两种样本
- `.so` 的 ZIP 压缩方式、ZIP 数据偏移和 ELF `LOAD` 段页大小对齐必须分别验证，不能互相替代
- 原 APK 为 `extractNativeLibs=true` 或未设置时，不得为了修复显式 `false` 场景而无条件改写 Manifest 或放大全部业务 Native 库
- 签名流程中的临时对齐不会破坏最终签名
- 如用户反馈 Google Play 16 KB 对齐问题，优先用同一 APK 复现并验证加固前后对齐差异

## Release 产物检查

- GitHub Release 只上传 GUI 安装包与校验和文件
- Linux 产物包含 AppImage / deb 与校验和
- macOS 产物包含 universal dmg 与校验和
- Windows 产物包含 NSIS 安装包与校验和
- 安装包内包含运行必需资源：`apktool.jar`、`apksigner.jar`、`resources.zip`
- 安装包内不包含测试 APK、测试证书、`shield.db`、`config.toml`、`.env` 或本地缓存
- Release Notes 已说明 Java 8+、证书管理、密码加密、16 KB 对齐和 macOS 未签名提示

## CLI 与核心库

- `cargo fmt --all --check` 通过
- `cargo clippy -p shield-core --all-targets -- -D warnings` 通过
- `cargo test -p shield-core` 通过
- `cargo test -p shield-cli` 通过
- `cargo test -p mocikashield` 通过
- `cd shield-stub && ./gradlew testDebugUnitTest --no-daemon` 通过；源码级 JVM 测试使用未混淆的 debug 变体，release 变体由资源构建、Stub DEX 指标和端到端加固回归覆盖
- `make build-stub` 能生成最新 `resources.zip`
- `make build-cli` 能生成 `shield` 二进制
- `shield protect -i input.apk -o protected.apk` 可完成基础加固

## 匿名使用统计与维护数据

- 启动、加固成功/失败、签名成功/失败均能写入本地每日汇总
- 启动后会上传当天累计快照；当天上传成功后本地记录仍保留
- 加固与签名事件在防抖窗口内合并上传，失败后不影响主流程并可在后续启动重试
- 采集请求携带项目 `User-Agent`，不会被 Cloudflare 按 Python 默认请求拦截
- 每日任务只更新 `stats` 分支中的 `data/history.json`，不生成公开页面或图表
- Traffic 或匿名统计接口不可用时明确记录不可用，不写成零，其他指标仍可继续保存

## 关键改动验证记录

### 2026-07-29：1.3.0 RC 发布前审计

| 项 | 结果 |
|----|------|
| Beta 后变更 | `v1.3.0-beta.1` 之后只有资源 ZIP 排除 macOS 元数据、发布说明结构和 AAB 实验结论文档变更；没有修改 DEXB v5、运行时协议、GUI 业务流程或证书数据库 |
| 开放问题 | 仅 Issue #2 保持开放；现有 API 23、API 35 16 KB 与 API 36 设备矩阵均已通过，当前没有新增可复现证据表明 Beta 存在阻塞回归，不主动重复打扰反馈用户 |
| 三平台发布 | Beta 与 RC Release workflow 的 Linux、macOS Universal、Windows 和发布汇总任务全部成功；RC Release 包含四个 GUI 安装包和三个 SHA-256 校验文件 |
| 发布说明 | 固定保留“下载 / Downloads”“使用须知 / Notes”“本次变更 / What's Changed”；自动变更列表改为以上一公开正式版本为比较基线，RC 将累计覆盖整个 Alpha、Beta 与 RC 周期 |
| 版本与文档 | 核心、CLI、标准/API 19 Stub、GUI 后端、Tauri 配置和前端包版本已同步为 `1.3.0`；路线图已冻结并完成 `1.3.0` 能力范围，AAB 正式支持独立安排在 `1.5.0` |
| 官方产物抽查 | 官方 Beta macOS Universal DMG SHA-256 为 `f063946c4939056cd623a40db3a0d43e826e0cc9ef5a5ec9bf66d7a685cb1604`；应用内包含标准/API 19 双资源与所需 JAR，未发现 `.DS_Store`、测试 APK、测试证书、`shield.db` 或 `config.toml` |
| Java 8 正式版回归 | 使用官方 Beta macOS `.app`，在 Amazon Corretto `1.8.0_452` 环境完成证书识别、APK 加固和 V2/V3 签名；原 APK 与加固 APK 证书 SHA-256 均为 `5876BFB8FAEB6A9485AE07980E4300D74AB406B2A6A3BB41D0D49CFBFE8D43F5` |
| Java 8 覆盖安装 | OnePlus 5 / LineageOS / API 35：原 ARouter APK 冷启动和路由扫描正常；同签名覆盖安装加固 APK 成功，首次冷启动 422 毫秒、缓存冷启动 244 毫秒，未出现 Java、签名、Native 加载或运行时崩溃异常 |

**正式版判断**：`v1.3.0-rc.1` 三平台构建、产物上传和累计发布说明均已通过，当前没有阻塞 `1.3.0` 的已知缺陷。正式版继承 Beta 完整功能回归和 RC 发布产物门禁，不重复执行未被触及的真机矩阵。

### 2026-07-29：1.3.0 Beta 功能冻结回归

| 项 | 结果 |
|----|------|
| 自动检查 | Rust 格式、核心 Clippy、核心 95 项、Native 15 项、Stub JVM 22 项、维护脚本 19 项测试全部通过；前端生产构建通过 |
| 双资源与静态治理 | 标准/API 19 双资源构建通过；API 19 ELF 审计通过；Stub DEX 指标守门通过 |
| 端到端加固 | 显式 `extractNativeLibs=false` 的双 DEX 样本完成编译、签名、加固、重签、签名校验、Native 库不压缩与 16 KB ZIP 对齐验证 |
| 普通环境严格策略 | OnePlus 5 / LineageOS / API 35 / arm64-v8a，普通 ADB 下未加固基线、严格策略首次解密、同签名覆盖安装和缓存命中二次启动均通过 |
| ADB Root 双向策略 | 同一设备开启 ADB Root 后，严格策略返回 `S01` 并拒绝启动；兼容策略完成首次解密和缓存命中启动 |
| 缓存恢复 | Root 下篡改 `c1.dex` 首字节后，下一次冷启动识别异常、重建缓存并恢复原 SHA-256；真实 Application、Activity 与第二 DEX 类全部加载成功，缓存文件恢复只读 |
| 私有目录边界 | 切回普通 ADB 后读取 `/data/user/0/dev.mocika.shield.smoke/app_app_dex` 被拒绝，符合不可调试正式包沙箱边界 |
| 启动耗时 | 缓存命中路径连续三次冷启动 `TotalTime` 为 185、177、184 毫秒；本轮用于建立当前设备基线，不作为跨设备性能承诺 |
| 历史矩阵 | Android 4.4、6.0、9、16 KB 与真实用户样本沿用 1.2.7 收尾审计中的有效证据；本轮没有对应模拟器在线，不记为重新执行 |

**Beta 判断**：缓存与 Root 策略新增风险面已经完成自动化和真机覆盖，功能与资源协议可以冻结。API 29 以上内存 DEX 实验保持关闭，不进入 Beta 运行路径。

### 2026-07-29：1.2.7 发布前收尾审计

| 项 | 结果 |
|----|------|
| 候选版本 | `v1.2.7-rc.5`；版本号在核心、CLI、GUI、标准 Stub 和 API 19 兼容 Stub 中保持一致 |
| 发布构建 | Linux、macOS Universal、Windows 三平台 Release 任务全部成功；Release 包含四个 GUI 安装包和三个 SHA-256 校验文件 |
| 官方安装包抽查 | 从 GitHub Release 下载 RC.5 macOS Universal DMG，只读挂载后确认应用内同时包含 `resources.zip` 与 `resources-api19.zip`；两包均只含预期 DEX、元数据和四 ABI Native 库，未夹带 `.DS_Store` |
| 标准资源 | `resources.zip` 存在，包含四种 ABI，元数据最低 API 为 21；标准模式正式支持 Android 5.0 及以上 |
| 工控兼容资源 | `resources-api19.zip` 存在，包含四种 ABI，元数据最低 API 为 19；`armeabi-v7a` 使用 r25c、Rust 1.77.2 和 API 19 构建，ELF 与动态符号审计通过 |
| Android 4.4～6.0 | 同一个兼容加固 APK 已在 API 19、21、23 模拟器完成首次安装、清除数据、覆盖安装、双 DEX 与真实 Application 回归；Android 6.0 工控真机完成详细回归，Issue #15 用户确认 Android 4.4.2 `armeabi-v7a`/NEON 工控板正常运行，但未逐项覆盖全部业务测试 |
| Android 9 | Issue #17 用户样本修复后已由用户在 Android 9 真机确认，未再出现共享库编译桩 `RuntimeException: Stub!` |
| ARouter | 用户多模块样本已在 Android 16 真机通过首次安装、缓存命中、清除数据、跨模块路由和参数注入回归 |
| Native 打包与 16 KB | `extractNativeLibs=false` 的有/无原始 Native 库样本已完成结构测试；API 23、API 35 16 KB 与 API 36 设备回归通过 |
| 每次启动安全检查 | 冷启动首次解密和缓存命中再次启动均通过，缓存路径不会绕过环境检查 |
| 未决事项 | Issue #2 等待用户提供原始/加固 APK 或具体不兼容库证据；Android 4.4 其他 CPU、厂商系统和特殊硬件场景按新增反馈单独验证 |

**正式版判断**：Android 4.4.2 工控真机已确认核心运行正常，`1.2.7` 的低版本兼容阻塞解除。发布说明必须限定已验证硬件为 `armeabi-v7a`/NEON，不能把未覆盖的其他 CPU、厂商系统和硬件业务描述为已经验证。

### 2026-07-27：Android 5.0～6.0 DEX 注入兼容原型

| 项 | 结果 |
|----|------|
| 自动验证 | API 21～22/23 方法路由与 Element 前插单元测试通过；混淆壳构建和端到端加固回归通过 |
| Android 5.0 | 官方 ARM64 AVD `mocika_api21`，Android 5.0.2（API 21）；单 DEX、双 DEX、首次安装、清除数据后启动和覆盖安装均成功；历史构建曾通过路径日志确认 Element 工厂分支，当前以真实 Application 与多 DEX 加载结果验收 |
| Android 6.0 | 官方 ARM64 AVD `mocika_api23`，Android 6.0（API 23）；单 DEX、双 DEX、首次安装、清除数据后启动和覆盖安装均成功；历史构建曾通过路径日志确认 Element 工厂分支，当前以真实 Application 与多 DEX 加载结果验收 |
| 高版本回归 | 一加 ONEPLUS A5000，Android 16（API 36），arm64-v8a；测试 APK 安装和冷启动成功 |
| 运行时路径 | 真实 Application 与多 DEX 类加载成功，进程持续存活，无 DEX 注入异常；历史构建曾通过日志确认 API 36 命中 `addDexPath` |
| 后续结果 | Android 6.0 工控真机已完成首次安装、清除数据、覆盖安装、多 DEX、Native 库和主要业务验证；标准模式最低支持范围已调整为 API 21 |

### 2026-07-20：严格 V2-only APK 签名提取

| 项 | 结果 |
|----|------|
| 测试环境 | macOS 桌面应用；华为 DBY-W09 真机，Android 12，arm64-v8a |
| 输入 APK | `app-release_v2-only-test.apk`，无有效 V1 签名，仅启用 V2 签名 |
| 加固与签名 | 桌面应用完成加固和自动签名，输出签名验证为 V2/V3，签名证书 SHA-256 与输入一致 |
| 安装启动 | ADB 安装成功；`MainActivity` 冷启动成功，`ComposeMainActivity` 热启动成功 |
| 运行时加载 | 三个原始 DEX 均完成解密和加载，两个页面中的独立模块组件正常绘制 |
| 异常检查 | 进程持续存活；无签名校验失败、解密失败、类加载失败或崩溃日志 |
| 加固状态预检 | `check-apk` 能识别载荷超过 4 KB 的 MSHD 追加块，`already_protected` 返回 `true` |

## 记录要求

每次正式发布前，在 Release 验收记录或 issue 中至少记录：

- 验证版本号和平台
- 使用的安装包来源
- Java 版本
- 测试 APK 类型和签名方案
- 是否完成签名、加固、自动签名、安装启动
- 发现的问题和处理结论

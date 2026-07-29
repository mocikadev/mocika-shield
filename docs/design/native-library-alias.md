# Native 库名称去品牌化与按任务别名设计

本文定义加固输出中 Native 壳库名称的去品牌化方案。目标是移除 `libmocikashield.so` 这一直接暴露工具身份的固定名称，并降低不同加固 APK 之间仅凭文件名建立关联的便利性。

该能力只治理明显静态特征，不改变 DEXB v5、加密算法、JNI 注册方式、ABI 策略或运行时安全边界。

## 背景与结论

当前资源包和加固输出在各 ABI 下使用固定名称：

```text
lib/<abi>/libmocikashield.so
```

解压 APK 后无需反编译即可识别 Mocika Shield。直接改成另一个固定名称只会形成新的固定指纹；使用纯十六进制随机名称又会与普通业务库命名风格明显不同，同样容易被判断为壳或动态加载组件。

最终采用以下策略：

- 构建资源内部继续保留一个明确的规范名称，便于构建、审计和故障定位。
- 每次加固生成一个固定长度、仅含小写字母的中性自然别名。
- 加固时同步重命名四 ABI Native 库，并等长修改 Stub DEX 中的加载名称。
- 不模仿原 APK 的品牌、包名或第三方 SDK，不覆盖或合并用户已有 `.so`。
- 最终 APK 不再出现 `mocikashield` 或构建期占位符。

## 目标与非目标

### 目标

1. 加固输出中不出现 `libmocikashield.so`。
2. 同一次加固的所有 ABI 使用同一个别名，不同加固任务通常使用不同别名。
3. 别名看起来像普通中性 Native 模块名称，不使用长十六进制串或明显递增编号。
4. 与原 APK 任意 ABI 下已有库名全局排重，不覆盖用户文件。
5. 保持 API 19 工控兼容、Android 5.0 以上标准模式、`extractNativeLibs=false`、16 KB ZIP 对齐和动态 JNI 注册行为不变。
6. 失败时在生成输出 APK 前终止，不留下部分改写产物。

### 非目标

- 不宣称隐藏 APK 使用了加固或动态加载技术。
- 不试图对抗基于 ELF 内容、JNI 行为、Stub DEX、加载时机或运行时 Hook 的专业识别。
- 不把壳代码合并进用户已有 Native 库，也不冒充某个业务模块或第三方 SDK。
- 不通过把 `.so` 移到 `assets` 后自行解压加载来规避标准 Native 打包规则。
- 不在 GUI 增加“自定义壳库名称”选项，避免用户制造非法名称或稳定指纹。

## 威胁模型与收益边界

| 识别方式 | 本方案效果 |
|----------|------------|
| 搜索 `libmocikashield.so` | 消除 |
| 按固定新名称批量匹配 | 每任务别名可降低关联性 |
| 观察异常长的随机十六进制库名 | 使用中性自然别名避免该特征 |
| 比较壳 ELF 内容或导入表 | 不解决 |
| 分析 `JNI_OnLoad`、DEX 注入和 Application 替换 | 不解决 |
| 运行时枚举加载库或 Hook | 不解决 |

因此该功能应描述为“Native 库名去品牌化”或“降低固定文件名特征”，不得描述为“无法识别加固”。

## 名称协议

### 规范名称与输出别名

构建生成的 `resources.zip` 和 `resources-api19.zip` 可继续使用内部规范名称 `libmocikashield.so`。这些资源位于 Mocika Shield 桌面程序内部，分析者已经知道工具身份，改名没有额外安全收益。

加固输出必须将规范名称映射为任务别名：

```text
内部资源：libmocikashield.so
任务别名：libnativecorebridge.so
```

规范名称不得直接复制进最终 APK。

### 固定长度占位符

Stub Java 构建时将加载名称改为固定 16 字节占位符：

```java
System.loadLibrary("mocikanativeslot");
```

`mocikanativeslot` 长度为 16 个 ASCII 字节。加固时只允许替换为同样 16 字节的小写字母别名，因此不改变 DEX 字符串长度、偏移和文件布局，只需重新计算 DEX SHA-1 与 Adler32。

资源元数据增加以下契约字段：

```json
{
  "native_library": "libmocikashield.so",
  "native_name_placeholder": "mocikanativeslot",
  "native_name_length": 16,
  "native_name_scheme": 1
}
```

标准资源与 API 19 资源必须声明相同协议；缺少字段、协议版本不支持或占位符数量不符合预期时，加固直接失败。

### 中性自然别名生成

别名主体由经过审查的中性词片段组合生成，总长度固定为 16，只使用 `a-z`：

```text
nativecorebridge
runtimeappbridge
nativeappsupport
commonruntimejni
```

完整文件名为 `lib<主体>.so`。候选通过系统安全随机源选择，不使用时间戳、任务序号或普通伪随机数。

词片段只表达通用技术角色，不包含：

- `mocika`、`shield`、`protect`、`packer`、`shell` 等工具或加固语义；
- 原 APK 包名、应用名称或证书信息；
- `opencv`、`bugly`、`flutter` 等第三方品牌；
- 系统库和常见运行库的完整名称。

生成器应提供足够大的合法组合空间，但必须承认组合语法本身仍可能被专业规则识别。该取舍优先保证名称自然、实现可靠和诊断可控，而不是制造虚假的不可识别承诺。

### 冲突与保留名称

加固前扫描原 APK 中全部 `lib/<abi>/*.so`，以不区分大小写的文件名集合做全局排重。即使名称只存在于一个 ABI，也视为冲突。

同时拒绝：

- 内部规范名称和构建期占位符对应名称；
- `libc.so`、`libdl.so`、`liblog.so`、`libm.so`、`libandroid.so` 等系统名称；
- 不符合 `^lib[a-z]{16}\.so$` 的候选；
- ZIP 中经过路径规范化后出现的重复条目。

最多尝试 64 次。超过上限应返回明确错误，不回退到固定名称或 `libmocikashield.so`。

## 加固流程

```text
读取原 APK 全部 Native 库名称
  → 解析资源元数据与占位符协议
  → 生成无冲突的 16 字节中性别名
  → 解压 Stub DEX 到任务临时目录
  → 验证 DEX 逻辑范围内占位符恰好出现一次
  → 等长替换加载名称并修复 DEX header
  → 将各 ABI 规范 Native 库写入别名路径
  → 验证规范名称和占位符已从输出资源消失
  → 追加 MSHD/DEXB v5 载荷并再次修复 DEX header
  → apktool 重打包
  → 按原 Manifest 策略处理 `.so` 压缩方式并执行 16 KB 对齐
  → 最终结构复验
```

### 顺序约束

1. 名称生成必须发生在 Runtime 资源注入前。
2. 占位符替换只扫描 Stub DEX 的逻辑 `file_size` 范围，不能误改之后追加的 MSHD 密文。
3. 四 ABI 复用同一个别名，不能为每个 ABI 单独生成。
4. ABI 过滤逻辑保持现状：原 APK 包含 Native 库时，只注入允许的 ABI；无 Native 库时按当前模式注入完整支持集合。
5. `extractNativeLibs=false` 场景仍须将全部 `.so` 写为 ZIP `Stored` 并按 16 KB 对齐。
6. 别名不是密码或密钥，不进入匿名统计，也不需要持久化到 `config.toml` 或 `shield.db`。

## 模块边界

### `shield-core`

新增独立的 Native 别名模块，负责：

- 从原 APK 收集并规范化已有库名；
- 生成和校验任务别名；
- 解析资源元数据中的名称协议；
- 等长替换 Stub DEX 占位符并修复 header；
- 在 Runtime 注入时把规范库路径映射为别名路径；
- 执行注入后与最终 APK 的结构复验。

`protect_api` 只编排阶段，不承载名称生成、DEX 字节修改或 ZIP 扫描细节。

### `shield-stub`

- Java 层只保留固定长度加载占位符。
- Rust Native 源码、JNI 动态注册和 `JNI_OnLoad` 不感知最终文件名。
- 标准和 API 19 构建继续输出内部规范名称，由 `shield-core` 在每次加固时改写。

### GUI 与 CLI

- 不增加用户配置字段和命令行参数。
- 旧请求与旧配置完全兼容。
- 任务日志只说明“Runtime Native 别名已生成”，默认不打印具体名称，避免日志成为跨系统传播的固定诊断依赖。

## 兼容性分析

### Android Native 加载

`System.loadLibrary()` 接收不含 `lib` 前缀和 `.so` 后缀的主体。Stub DEX 与 APK 内文件名同步后，Android 仍按正常 Native 库规则解析；Rust `cdylib` 的 JNI 注册逻辑不依赖外部文件名。

实现前必须审计四 ABI ELF 动态段。如果产物存在固定 `DT_SONAME`，应确认 Android 加载与升级行为不依赖它；不得未经验证直接重写 ELF 动态段。

### 覆盖安装与缓存

不同加固任务可以产生不同别名。APK 覆盖安装会替换基础包，应用进程重启后按新 Stub DEX 加载新名称；DEX 私有缓存不以 Native 库名为键，因此无需迁移缓存。

必须验证相同证书覆盖安装、版本升级和降级失败路径，确认旧进程、旧解压目录或厂商链接器缓存不会命中旧名称。

### Android 4.4 与 16 KB

- API 19 兼容库只改 ZIP 条目名称，不改 ELF 内容和 NDK/Rust 工具链。
- API 19、21、23 均须验证 `System.loadLibrary()` 能解析任务别名并进入 `JNI_OnLoad`。
- API 35 16 KB 与 API 36 继续分别验证 ZIP 偏移、压缩方式和 ELF `LOAD` 段，不能因名称变化省略任何一项。

## 失败策略

以下情况必须失败关闭，不生成输出 APK：

- 元数据没有名称协议字段，或标准/兼容资源协议不一致；
- Stub DEX 找不到占位符，或占位符出现次数不是一次；
- 无法从安全随机源生成名称；
- 64 次内无法得到无冲突候选；
- 任一需要注入的 ABI 缺少规范 Native 库；
- 注入后仍存在规范名称或占位符；
- 四 ABI 的别名不一致；
- 重打包后别名库的压缩方式、ZIP 对齐或签名复验失败。

不得静默回退到固定品牌名称，因为这会让同一版本的安全特征不可预测。

## 测试与验收

### 单元测试

- 所有生成主体长度均为 16，且只包含小写字母。
- 词片段组合不包含品牌、加固语义、第三方品牌和系统保留名称。
- 跨 ABI 同名、大小写差异、异常 ZIP 路径均能触发冲突。
- 固定随机输入可复现候选选择，安全随机源失败能够正确返回错误。
- DEX 占位符恰好一次时替换成功，并正确更新 SHA-1 与 Adler32。
- 占位符缺失、多次出现、长度不匹配时失败关闭。

### 无设备端到端测试

- 原 APK 无 Native 库、有单 ABI 库和多 ABI 库三类夹具。
- 最终 APK 不包含 `libmocikashield.so`、`mocikanativeslot` 或其他构建期标识。
- 所有注入 ABI 使用相同别名，原 APK 业务库名称和内容保持不变。
- 连续两次加固通常产生不同别名与不同 DEXB 密文。
- `extractNativeLibs=false` 时别名库和业务库均为 `Stored` 且 16 KB 对齐。
- 标准与 API 19 资源均通过同一套协议契约测试。

### 设备矩阵

| 环境 | 必测路径 |
|------|----------|
| API 19 `armeabi-v7a` | 首次安装、冷启动、清除数据、覆盖安装、双 DEX、真实 Application、Native 加载 |
| API 21 / API 23 | ART Element 工厂路径、缓存命中、覆盖安装、业务 Native 库共存 |
| API 28 | 系统共享库优先级和业务 Native 库共存 |
| API 35 16 KB | `extractNativeLibs=false`、ZIP 16 KB 对齐、Native 直接映射 |
| API 36 真机 | 首次解密、缓存命中、ARouter、每次启动安全检查、覆盖安装 |

### 完成条件

1. 最终 APK 静态扫描不再出现 `mocikashield`、规范库名或占位符。
2. 不同加固任务不依赖同一个固定输出库名。
3. 原 APK 所有业务 Native 库保持名称、ABI、内容和压缩策略语义。
4. 标准与 Android 4.4 兼容模式完成自动测试和设备矩阵。
5. 未引入新的 GUI 配置、DEXB 格式版本或用户迁移要求。
6. README 只说明 Native 库名按任务去品牌化，不公开内部词片段表和占位符实现细节。

## 实施顺序与版本边界

该能力不进入已经发布的 `1.2.7`，安排为 `1.3.0-alpha.1` 的独立交付。`1.2.7` 的线上观察与本能力开发并行，不构成启动条件；只有发现可稳定复现的高优先级回归时才临时抢占：

1. 先提交资源元数据协议、名称生成器和纯 Rust 单元测试。
2. 再实现 Stub DEX 等长替换与 Runtime 路径映射，补齐无设备端到端测试。
3. 完成标准资源和 API 19 资源构建审计。
4. 按设备矩阵验证后才允许进入正式版本；任一低版本加载或 Native 打包回归都应停止发布。

这项能力可以与后续资源能力协议共用版本协商机制，但必须作为独立提交和独立回归项，不与缓存完整性、Root 策略或内存 DEX 混在同一次实现中。

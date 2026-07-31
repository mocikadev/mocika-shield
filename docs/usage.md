# Mocika Shield - 使用指南

## GUI 用法（推荐）

从 [Releases](https://github.com/mocikadev/mocika-shield/releases/latest) 下载对应平台的安装包，安装后直接使用。

> **macOS 首次打开（未签名版本）**
>
> macOS 会提示「无法验证开发者」，在终端执行以下命令去除隔离标记，执行后正常双击打开即可，只需操作一次：
> ```bash
> xattr -rd com.apple.quarantine /Applications/MocikaShield.app
> ```

正式 GUI 为 Tauri 版，Linux / macOS / Windows 使用同一套界面。当前界面以加固、签名、证书、设置、关于为主，签名证书已从设置页拆出，由证书页统一管理。

- **加固**：拖入或选择 APK → 点击加固 → 实时进度 → 自动生成 `{name}_protected.apk`
- **签名**：拖入或选择 APK → 选择证书 → 点击签名；签名成功后只保留“继续签名”入口
- **证书**：导入已有 keystore / p12，或创建新的 PKCS12 证书；可设置默认证书
- **设置**：配置主题、语言与应用级选项
- **关于**：显示版本号、构建信息、Java 环境状态、检查更新，并支持手动重新检测环境和复制诊断信息

### 适用场景

- **优先使用 GUI**：日常加固、重新签名、管理签名证书
- **使用 CLI**：批处理脚本、CI 流水线、本地调试加固过程

### 首次使用建议流程

1. 先在 **证书** 页面导入已有 keystore / p12，或创建新的 PKCS12 证书
2. 将常用证书设为默认
3. 返回 **加固** 页面选择已签名 APK
4. 需要直接得到可安装产物时，使用默认启用自动签名的证书
5. 只做重签名时，使用 **签名** 页面并选择证书

当前版本签名资料由证书页统一维护：

- 导入证书保存前会校验 keystore 密码、alias 与证书可用性
- 创建证书默认生成 PKCS12 keystore，并保存到应用数据目录 `keystores/`
- 创建证书时 Keystore 密码至少 6 位；Key 密码可留空，填写时同样至少 6 位
- PKCS12 证书的 Alias 可能会被 `keytool` 规范为小写；GUI 会按大小写不敏感方式校验，并保存 keystore 中实际返回的 Alias
- 已保存证书的材料不可直接编辑；编辑入口只用于修改名称、备注、签名版本和自动签名偏好
- 如需更换 keystore 文件、Alias、类型或密码，请重新导入或创建一条证书记录
- 加固页会先校验原 APK 与默认证书指纹，一致时才执行加固和自动签名
- 签名页不会维护临时签名配置，只从证书列表中选择
- 最终产物为 `{name}_protected_signed.apk`
- GUI 内部会在输出前自动完成 APK ZIP 对齐，无需手动运行 `zipalign`

### 签名材料准备

建议提前准备以下材料：

- 已签名的原始 APK
- `keystore` / `p12`
- `alias`
- `keystore` 密码
- `key` 密码（相同可留空）

GUI 会在选择 APK 时比对原 APK 与默认证书的指纹，并在后端开始加固前再次校验。指纹不一致或证书读取失败都会阻止加固，因为改用其他证书签名会导致运行时无法解密和启动。

加固页提供“运行系统兼容性”选项。默认使用“Android 5.0 及以上”标准模式；目标设备包含 Android 4.4 工控板时，可选择“兼容 Android 4.4”。标准模式已覆盖 Android 5.0～6.0 的旧版 ART 注入路径，并包含 ARouter 运行期扫描和 Android 9 `org.apache.http.legacy` 兼容处理。兼容模式当前仅接受不含 Native 库，或 Native 库仅包含 `armeabi-v7a` 的 APK；后端会再次检查 ABI，并从应用内置的固定兼容资源包加固，前端不能传入任意资源路径。该选择只属于当前任务，不修改 `config.toml`，旧版请求未携带此字段时仍按标准模式执行。Android 4.4.2 `armeabi-v7a`/NEON 工控真机已确认能够正常运行；完整业务测试未全部覆盖，其他 CPU、厂商系统和硬件交互仍需按实际设备验证。

加固页同时提供“运行环境保护”选项：

| 模式 | 默认值 | 行为 | 适用建议 |
|------|--------|------|----------|
| 兼容模式 | 是 | 始终保留反调试检查；Root 信号不阻止应用启动 | 普通应用、模拟器、工控设备以及无法确认设备环境时使用 |
| 严格环境保护 | 否 | 反调试或高置信 Root、ADB Root、注入信号命中时拒绝启动 | 仅用于明确不允许 Root 环境运行的受控部署场景 |

运行环境策略属于当前加固任务的快照，不写入 `config.toml`。任务开始后不能修改；如需切换策略，必须重新选择并加固 APK，运行时不会从严格模式静默降级。严格策略只能提高常见提取和分析成本，不能承诺抵御隐藏 Root、检测绕过、内核级控制或进程内提取。若目标设备出现误判，请改用兼容模式重新加固，并提供设备系统及 Root 方案信息协助排查。

GUI 会在应用启动时检测一次本机 Java 环境，并将结果缓存到全局状态中；若未检测到完整 JDK 8+，或缺少 `keytool`，加固、签名、Alias 识别会直接阻断并给出明确提示。运行流程不依赖 `javac`。
如果应用启动后你又安装或切换了 JDK，可在关于页手动点击“重新检测环境”刷新状态。

### 配置与证书数据位置

GUI 启动时会一次性加载应用配置与证书数据库，运行期间使用同一份内存状态，不会在页面切换时反复从磁盘读取。应用级配置写入 `config.toml`；证书列表、默认证书、签名密码与校验状态写入本地 SQLite 数据库 `shield.db`。密码字段以 `enc:v1` 格式加密落盘；旧明文记录不再兼容，如遇到旧测试数据请重新导入或创建证书。

| 平台 | 应用配置 | 证书数据库 |
|------|----------|------------|
| Linux | `~/.config/dev.mocika.shield-gui/config.toml` | `~/.local/share/dev.mocika.shield-gui/shield.db` |
| macOS | `~/Library/Application Support/dev.mocika.shield-gui/config.toml` | `~/Library/Application Support/dev.mocika.shield-gui/shield.db` |
| Windows | `%APPDATA%\\dev.mocika.shield-gui\\config.toml` | `%APPDATA%\\dev.mocika.shield-gui\\shield.db` |

应用数据目录还会新增：

- `shield.db`：证书列表、默认证书、签名密码、校验状态
- `keystores/`：应用内新建或托管的 keystore 文件

---

## CLI 用法

### 基础用法

```bash
shield protect -i input.apk -o protected.apk
```

运行 CLI 前请先确认本机已安装完整 JDK 8+，且 `java`、`keytool` 可执行。

详细日志输出：

```bash
shield protect -v -i input.apk -o protected.apk
```

签名命令会先执行内置 ZIP 对齐，再调用发布包中的 `apksigner.jar`：

```bash
MOCIKA_SHIELD_KS_PASS='keystore密码' \
MOCIKA_SHIELD_KEY_PASS='key密码' \
shield sign \
  -i protected.apk \
  -o protected-signed.apk \
  --ks release.jks \
  --key-alias release
```

未设置 `MOCIKA_SHIELD_KEY_PASS` 时，Key 密码默认沿用 Keystore 密码。也可以使用 `--ks-pass` 和 `--key-pass` 显式传入，但自动化环境优先使用环境变量，避免密码直接出现在命令历史中。

### CLI 配置文件

CLI 人工配置建议固定命名为 `shield-cli.toml`，与 GUI 自动维护的 `config.toml` 完全独立。当前格式版本为 `1`：

```toml
schema_version = 1

[protect]
input = "input.apk"
output = "build/protected.apk"
environment_policy = "compatible"

[sign]
input = "build/protected.apk"
output = "build/protected-signed.apk"
keystore = "release.jks"
key_alias = "release"
keystore_type = "jks"
v1 = true
v2 = true
v3 = true
v4 = false
```

配置中的相对路径以配置文件所在目录为基准。命令行参数优先于配置文件：

```bash
shield --config shield-cli.toml protect
MOCIKA_SHIELD_KS_PASS='keystore密码' shield --config shield-cli.toml sign
shield --config shield-cli.toml protect -i another.apk -o another-protected.apk
```

配置文件不接受密码字段，密码只能通过环境变量或当前命令参数提供；CLI 不会把密码写入进度、错误或调试输出。

### 机器可读输出与退出码

`protect`、`sign` 增加 `--json` 后，每行输出一个独立 JSON 事件，事件类型固定为 `progress`、`done` 或 `error`。原有 `--json-progress` 继续作为 `protect --json` 的兼容别名。

- 成功退出码为 `0`，并以 `done` 事件结束。
- 参数、配置、加固、对齐或签名失败的退出码为 `1`。
- JSON 模式下错误写入标准输出的 `error` 事件；普通模式错误写入标准错误。
- 调试日志不会混入 JSON 标准输出。

### 完整流程

加固完成后 APK 未签名，需手动签名后才能安装：

```bash
# 1. 加固
shield protect -i input.apk -o protected.apk

# 2. 签名（无需额外执行 zipalign）
MOCIKA_SHIELD_KS_PASS='keystore密码' \
shield sign -i protected.apk -o protected-signed.apk \
  --ks keystore.jks --key-alias alias

# 3. 安装
adb install -r protected-signed.apk
```

### 查看帮助 / 版本

```bash
shield --help
shield --version
```

---

## 验证加固结果

```bash
# 检查 APK 结构（加固后无 assets/app.bin，加密数据藏在 classes.dex 末尾）
unzip -l protected.apk | grep -E "libmocikashield|classes"
```

应看到：
- `lib/<abi>/libmocikashield.so` — Rust 解密库（四个架构）
- `classes.dex` — 壳 DEX（体积很小，末尾追加了加密数据，工具不可见）

```bash
# 对比体积（protected.apk 通常比 input.apk 更小，因 Zstd 压缩率高）
ls -lh input.apk protected.apk
```

---

## 常见问题

### 找不到 apktool.jar / resources.zip

- **发布包**：jar 已内置，确保发布包目录结构完整（`lib/`、`resources/` 在 `bin/` 同级父目录下）
- **开发环境**：先执行 `make build-stub`，jar 在项目根 `tools/` 目录下

### 加固后 APK 崩溃

1. 查看日志：
   ```bash
   adb logcat | grep -E "AndroidRuntime|ax|dx|lx|rx|e[1-4]"
   ```

   当前壳层日志 tag 已做弱特征化处理，常见 tag 为 `ax`（StubApp）、`dx`（DEX 注入）、`lx`（Ld）、`rx`（ARouterCompat）。

2. 确认未用未签名 APK 加固（必须先签名再加固）：
   ```bash
   java -jar apksigner.jar verify input.apk
   ```

   `apksigner verify` 退出码为 `0` 表示 APK 已签名；V2/V3/V4 签名不一定会在 `META-INF/` 下留下证书文件。

3. 确认设备架构与注入的 so 匹配：
   ```bash
   unzip -l protected.apk | grep libmocikashield.so
   adb shell getprop ro.product.cpu.abi
   ```

4. 标准模式按 Android 5.0（API 21）及以上设计：API 21、23 已通过官方 ARM64 模拟器回归，Android 6.0 工控设备已验证首次安装、清除数据、覆盖安装、多 DEX、Native 库和主要业务功能；Android 4.4（API 19～20）须选择“Android 4.4 工控兼容”模式，当前已验证 Android 4.4.2 `armeabi-v7a`/NEON 工控设备

### 能否重复加固？

不可以。GUI 会在选择文件时提示，核心加固入口也会在解包和创建输出前再次检测，因此 GUI 和 CLI 都无法对已加固 APK 重复操作。请始终使用原始未加固的 APK。

### 为什么证书页保存后其他页面会立即生效？

GUI 只维护一份全局证书状态。证书页保存、删除或切换默认证书后，会同时更新内存状态和本地 `shield.db`，加固页、签名页会立即复用最新证书列表。

### 反馈问题时需要提供什么？

如果需要在 GitHub issue 中反馈加固、签名或环境检测问题，建议先在 **关于** 页面点击“复制诊断信息”，并将内容粘贴到 issue 中。诊断信息只包含版本、平台、Java 状态、工具状态和配置/数据目录可用性，不包含 APK 路径、证书路径、密码或完整用户目录。

### 加固后为什么体积反而变小？

DEX 文件经 Zstd level 19 压缩后体积大幅减小，通常比原始 APK 更小。

---

## 性能参考

典型压缩率（Zstd level 19）：

| 文件 | 原始大小 | 压缩后 | 压缩率 |
|------|---------|--------|--------|
| classes.dex | 30 MB | 4.2 MB | 14% |
| classes2.dex | 12 MB | 3.7 MB | 30% |
| classes3.dex | 6.7 MB | 1.9 MB | 29% |

- 首次启动额外耗时：50–100 ms（单次 JNI 解密 + 解压，后续命中缓存跳过）
- Runtime 内存占用：约 1–2 MB

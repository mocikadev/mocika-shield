# Mocika Shield - 使用指南

## GUI 用法（推荐）

从 [Releases](../../releases) 下载对应平台的安装包，安装后直接使用。

> **macOS 首次打开（未签名版本）**
>
> macOS 会提示「无法验证开发者」，在终端执行以下命令去除隔离标记，执行后正常双击打开即可，只需操作一次：
> ```bash
> xattr -rd com.apple.quarantine /Applications/MocikaShield.app
> ```

正式 GUI 为 Tauri 版，Linux / macOS / Windows 使用同一套界面。界面包含四个页面：

- **加固**：拖入或选择 APK → 点击加固 → 实时进度 → 自动生成 `{name}_protected.apk`
- **签名**：拖入或选择 APK → 使用设置页保存的签名配置 → 点击签名
- **设置**：配置唯一正式签名信息（keystore / alias / 密码 / 签名版本）、主题、语言
- **关于**：显示版本号、构建信息、检查更新

签名配置只有一份，保存在设置页中：

- 加固页开启“自动签名”后，会在加固完成后直接使用这份配置继续签名
- 签名页不会再单独维护临时配置，只读取设置页中保存的正式配置
- 最终产物为 `{name}_protected_signed.apk`

---

## CLI 用法

### 基础用法

```bash
shield protect -i input.apk -o protected.apk
```

详细日志输出：

```bash
shield protect -v -i input.apk -o protected.apk
```

### 完整流程

加固完成后 APK 未签名，需手动签名后才能安装：

```bash
# 1. 加固
shield protect -i input.apk -o protected.apk

# 2. 签名（使用 apksigner）
java -jar apksigner.jar sign \
  --ks keystore.jks \
  --ks-key-alias alias \
  --out protected-signed.apk \
  protected.apk

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
   adb logcat | grep -E "AndroidRuntime|ax|lx|rx|e[1-4]"
   ```

   当前壳层日志 tag 已做弱特征化处理，常见 tag 为 `ax`（StubApp）、`lx`（Ld）、`rx`（ARouterCompat）。

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

4. 确认 Android 版本 ≥ 7.0（API 24）

### 能否重复加固？

不可以。GUI 和 CLI 均会检测已加固的 APK 并阻止重复操作。请始终使用原始未加固的 APK。

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

# ROADMAP.md — Mocika Shield 功能路线图

> 记录待修复缺陷与待实现功能。技术细节见 [internals.md](../design/internals.md)。
> 最后更新：2026-07-07

---

## 说明

- 状态：`待修复` / `待实现` / `进行中` / `已完成` / `已否决`
- 优先级：`高` / `中` / `低`

---

## 一、已知缺陷（Bug）

### V2/V3-only APK 签名预检误判

| 项 | 内容 |
|----|------|
| **优先级** | 高 |
| **状态** | 已完成 |
| **涉及文件** | `apps/shield-cli/src/main.rs`、`apps/shield-gui/src-tauri/src/main.rs` |

**现象**：用 V2/V3 签名（无 V1）的 APK 拖入 GUI，预检会显示"未签名"，阻止后续流程。

**根因**：`do_check_apk` 只检测 `META-INF/*.RSA|DSA|EC` 是否存在来判断签名状态。这是 V1 签名的特征，V2/V3 签名存储在 APK Signing Block（ZIP 中央目录前的扩展区域），该代码完全检测不到。

**最终修复方案**：

**CLI 层（`apps/shield-cli/src/main.rs`）**：
1. 新增 `has_v2_v3_signature()`：读取 APK 末尾 64KB，扫描 `"APK Sig Block 42"` magic（16 字节），命中即判定 V2/V3 已签名
2. `check_apk_json()` 在 V1 未检到时 fallback 调用 `has_v2_v3_signature()`
3. `extract_apk_cert_fingerprint()` 先试 `keytool -jarfile`（V1），失败则 fallback `apksigner verify --print-certs`（V2/V3），新增 `parse_sha256_from_apksigner()` 解析输出格式

**Tauri GUI**：`do_check_apk` 调用 `apksigner verify <apk_path>` 判断退出码（0 = 已签名）；`apksigner` 通过 `find_apksigner_path` 查找，无法找到时降级 V1 检测并提示。

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

检测逻辑放在 Rust native 层 `f2` 入口，先于所有解密动作。触发时 `Log.w("dbg")` + 抛 `RuntimeException("dbg")`，不透露具体原因。无额外 Cargo 依赖（纯 `std::fs`）。

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

同时支持 `--config <mocika-shield.toml>` 从文件读取参数，命令行优先级高于配置文件，方便 CI 复用。CLI 的人工配置使用 TOML；GUI 自动维护的配置已统一为 `config.toml`。

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

**仍缺失**：

- `protect` 端到端 smoke test（输入 APK → 输出结构验证）
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
| native 库名 | `mocikashield`（不可避免，与 .so 文件名绑定） |

---

## 三、已否决

| 功能 | 原因 |
|------|------|
| 签名密钥系统级存储（Keychain 等） | 每次使用需认证，体验差 |
| resources.zip 加密 | 密钥必须内置工具中，安全收益极低 |
| 加固历史记录 | 实际意义不大 |

---

*最后更新：2026-07-07*

# AGENTS.md — Mocika Shield

Android APK 加固工具。`crates/shield-core` 提供共享核心能力，`apps/shield-cli`（Rust 命令行）负责命令入口，`shield-stub`（Android 壳模块）在运行时解密加载 DEX，`apps/shield-gui`（Tauri v2 桌面 GUI）提供跨平台图形化加固与签名界面。

## 语言约定

**所有交流、代码注释、提交信息、文档均使用中文。**

> ⚠️ AI 强制检查点：每次回复前必须确认以下两项，违反则本次回复无效：
> 1. 回复语言 = 简体中文（不得出现韩文、日文或其他非中文内容）
> 2. 提交信息格式 = `<英文类型>: <中文描述>`，类型限于 `feat/fix/docs/style/refactor/perf/test/build/ci/revert/chore`

---

## 技术栈

- `crates/shield-core`：Rust 共享核心库，加固、签名、对齐、环境探测
- `apps/shield-cli`：Rust + clap，单一二进制 `shield`，仅承担命令行入口与输出适配
- `shield-stub`：Android（Java 壳 + Rust Native JNI，`libmocikashield.so`）
- `apps/shield-gui`：Tauri v2，唯一正式桌面 GUI，目标三平台（Linux/macOS/Windows）
- GUI 前端：React + TypeScript + Vite + Tailwind CSS + shadcn/ui + lucide-react
- 构建：Makefile + Gradle（AGP 8.x / Kotlin 2.x）+ cargo-ndk + npm + tauri-cli

---

## 常用命令

```bash
make build-stub             # 构建 Android 壳（必须最先执行，输出 resources.zip）
make build-cli              # 编译 shield 二进制
make build-gui              # 构建桌面 GUI Tauri 版（需先 build-stub）
make build-all              # build-stub + build-cli + build-gui
make release VERSION=x.y.z  # CLI-only 发布包（维护者本地使用）
VERSION=x.y.z make release-linux        # Linux Tauri 发布包
VERSION=x.y.z make release-macos        # macOS Tauri 发布包
make test                   # 运行 shield-core + shield-cli 单元测试
make clean                  # 清理所有构建产物
```

Windows 发布需在 Windows 原生环境执行：

```powershell
.\scripts\release-windows.ps1 -Version x.y.z
```

---

## 关键约束

### 构建顺序
`shield`（CLI）和 GUI 运行时均依赖 `resources.zip`，**必须先 `make build-stub`**。

### NDK 版本
`shield-stub/build.gradle.kts` 硬编码 NDK `29.0.14206865`；版本不同需修改或设置 `ANDROID_NDK_ROOT`。

Android 4.4 实验兼容构建固定使用 NDK r25c `25.2.9519653`、Rust `1.77.2` 和 API 19，不得替换标准构建的 r29。验证入口为 `scripts/verify-android-api19-native.sh` 与 `tests/scripts/run-api19-native-probe.sh`。

### Rust targets
首次编译 `shield-stub` 前需：
```bash
rustup target add aarch64-linux-android armv7-linux-androideabi i686-linux-android x86_64-linux-android
```

### 版本号同步
升级时优先使用 `scripts/bump-version.sh`，同步 `crates/shield-core/Cargo.toml`、`apps/shield-cli/Cargo.toml`、`shield-stub/src/main/rust/Cargo.toml`、`shield-stub/compat/api19-rust/Cargo.toml` 及其独立 `Cargo.lock`、`apps/shield-gui/src-tauri/Cargo.toml`、`apps/shield-gui/src-tauri/tauri.conf.json`、`apps/shield-gui/package.json` 和 `apps/shield-gui/package-lock.json`。API 19 发布构建使用 `--locked`，不得只修改兼容清单而遗漏独立锁文件。

### DEXB v5 格式
加密 DEX 以 MSHD 块追加到 `classes.dex` 末尾（DEX `file_size` 之外，工具不可见）。
DEXB 头部明文区布局：`magic(4) + version(4) + dex_count(4) + sig_len(1) + signature[sig_len] + ikm_len(1) + ikm[ikm_len] + nonce(12)`，之后为 ChaCha20-Poly1305 密文。
当前 stub 仅支持 v5；v5 与 v4 不兼容，旧加固 APK 需重新加固。

### 签名校验
`extractAndDecryptFromDex(ctx, dex, key)` **必须传 `Context ctx`**（`attachBaseContext` 阶段 `ActivityThread.currentApplication()` 返回 null）。

### ProGuard JNI 规则
所有被 JNI Native 回调的 Java 类必须用 `{ *; }` 全保留（R8 无法感知 JNI 调用，逐条列举会漏掉方法）。

### Windows 路径规范化
Windows 下 `current_exe()`、`ProjectDirs`、Tauri `resource_dir()` 等接口可能返回 `\\?\C:\...` 或 `\\?\UNC\...` 格式的扩展 UNC 路径，Java 不支持此格式。
统一使用 **`dunce::simplified()`** 规范化，不要手写字符串替换。

### Windows 子进程无控制台窗口
调用 `java`、`keytool`、`apksigner` 等子进程时，必须通过 `no_window_command(prog)` 辅助函数创建 `Command`，该函数在 `#[cfg(target_os = "windows")]` 下设置 `CREATE_NO_WINDOW` flag（其他平台零开销）。

### Native 库打包
原 APK 显式设置 `android:extractNativeLibs="false"` 时，加固输出必须保留该值，并确保全部 `lib/**/*.so` 使用 ZIP `Stored` 不压缩存储且按 16 KB 对齐。不得通过无条件改成 `true` 规避安装失败；签名链路继续保留原压缩策略。

### shield-gui 后端结构
`apps/shield-gui/src-tauri` 是 Tauri binary crate，`main.rs` 只做启动、状态注入和 command 注册。后端逻辑按职责拆到 `app_config.rs`、`cert_service.rs`、`signing.rs`、`protect_runner.rs`、`updates.rs` 等模块；不要再把业务逻辑堆回 `main.rs`。

### GUI 维护策略
正式开源版本只维护一份桌面 GUI：`apps/shield-gui`（Tauri + React）。

### 本机测试交付规则
凡是面向用户本机直接测试 GUI 效果、交互、签名、加固流程的构建，默认必须提供 **macOS `.app` 应用包**（如当前在 macOS 环境）。不要只交付裸二进制 `target/release/mocika-shield`，因为其运行时资源不完整，不能代表真实桌面应用形态。

### 配置文件命名
GUI 自动维护的应用级配置固定使用 `config.toml`。证书列表、默认证书与签名密码等结构化数据统一放到 GUI 本地 SQLite 数据库 `shield.db`。如未来增加 CLI 人工配置文件，必须与 GUI 自动维护的 `config.toml` 明确区分，不得复用同一个文件。

### 证书与密码持久化策略
本工具定位为**本地离线桌面工具**，证书管理以“省事、减少重复输入”为优先目标。GUI 允许在本机持久化保存签名证书相关数据，包括 `keystore_password` 与 `key_password`；默认不引入系统 Keychain 作为前置依赖。

约束如下：

- 应用级配置放 `config.toml`
- 证书与签名资料放 `shield.db`，密码字段必须以 `enc:v1` 格式加密落盘，不兼容旧明文记录
- 应用内新建 keystore 默认放应用数据目录下的 `keystores/`
- 导入已有 keystore 默认只记录原始路径，不强制复制
- 创建证书时 Keystore 密码至少 6 位；Key 密码可留空，填写时同样至少 6 位
- PKCS12 Alias 可能被 `keytool` 规范为小写，后端校验必须大小写不敏感，并保存 keystore 实际返回的 Alias
- 证书材料保存后视为不可变：`keystore` 文件、类型、Alias、密码不得通过“编辑证书”修改；如需更换材料，应重新导入或创建证书
- “编辑证书”只允许修改显示名称、备注、签名版本、自动签名偏好和默认项
- Tauri 前端证书列表不得持有密码明文；签名、自动签名、证书指纹比对只传证书 ID，由后端读取并解密
- 任何日志、错误信息、调试输出都不得打印密码明文
- 涉及密码落盘的文件应尽量收紧权限

### 前端路径拼接
前端构造输出路径时必须通过 `src/lib/path.ts` 的路径辅助函数处理 Windows 反斜杠，不要用裸 `format!("{}/{}", parent, stem)` 或字符串拼接散落在页面里。

### 提交前合并同类提交
每次提交前，先查看最近几条 `git log`，若末尾存在与本次**同类型（type 相同）且主题相近**的提交，优先用 `git rebase -i` 将其合并（squash）为一条，再追加当前变更一并提交。避免积累大量碎片化提交。

### 提交信息禁止内部追踪编号
提交信息（commit message）和文档中**不得出现内部路线图编号**。描述功能时直接说清楚做了什么，不引用编号。

---

## 仓库配置规范

### 仓库关系

本项目为开源仓库，远程地址统一为 `git@github.com:mocikadev/mocika-shield.git`。

发布时通过 GitHub Actions 或本地发布脚本生成构建产物，并上传到同一仓库的 GitHub Releases。

### About Description

格式：`中文描述 · English description`（用 ` · ` 分隔，单行）

| 仓库 | description |
|------|-------------|
| `mocikadev/mocika-shield`（GitHub） | `Android APK 加固工具（DEX 加密 + 壳保护 + 反调试） · Android APK hardening — DEX encryption, stub loader & anti-debug` |

### Homepage URL

| 仓库 | homepage |
|------|----------|
| `mocikadev/mocika-shield` | `https://mocikadev.github.io/mocika-shield/` |

### Topics

| 仓库 | topics |
|------|--------|
| `mocikadev/mocika-shield` | `android` `apk` `security` `rust` `tauri` `react` `apk-protection` `dex-encryption` `android-security` |

---

## 文档导航

| 文档 | 内容 |
|------|------|
| [README.md](README.md) | 用户快速开始、工作原理概览 |
| [docs/README.md](docs/README.md) | 文档总览、模块概览、阅读路线 |
| [docs/ops/build.md](docs/ops/build.md) | 从源码编译的详细步骤（含 GUI、Windows） |
| [docs/ops/environment.md](docs/ops/environment.md) | 环境要求与 NDK 配置（含 Windows Scoop 一键配置） |
| [docs/ops/project-statistics.md](docs/ops/project-statistics.md) | 项目关注度统计口径、自动化流程与维护方式 |
| [docs/usage.md](docs/usage.md) | CLI 与 GUI 使用指南 |
| [docs/design/internals.md](docs/design/internals.md) | 技术内参：DEXB v5 格式、加解密算法、已知 Bug 全记录 |
| [docs/design/native-library-packaging.md](docs/design/native-library-packaging.md) | Native 库打包：`extractNativeLibs`、ZIP 压缩/对齐与 ELF 页大小兼容方案 |
| [docs/design/native-library-alias.md](docs/design/native-library-alias.md) | Native 库名去品牌化：任务别名、DEX 联动改写、冲突规避与兼容回归方案 |
| [docs/design/stub-dex-minimization.md](docs/design/stub-dex-minimization.md) | Stub DEX 最小化：能力变体、职责下沉、二阶段加载实验与停止条件 |
| [docs/design/runtime-security.md](docs/design/runtime-security.md) | Android 运行时安全：每次启动检查、DEX 缓存认证、Root 策略与内存 DEX 规划 |
| [docs/design/dex-code-separation.md](docs/design/dex-code-separation.md) | DEX 代码保护研究：结构/方法代码分离、内存重建、分片恢复与停止条件 |
| [docs/design/android-4.4-compatibility.md](docs/design/android-4.4-compatibility.md) | Android 4.4 工控兼容：双 NDK、ABI、运行时分流与验证方案 |
| [docs/design/architecture.md](docs/design/architecture.md) | 完整目录结构、工具路径检测逻辑、构建产物路径 |
| [docs/design/gui.md](docs/design/gui.md) | GUI 设计：唯一正式桌面 GUI、页面结构、签名配置与维护约束 |
| [docs/process/commit-convention.md](docs/process/commit-convention.md) | Commit Message 规范 |
| [docs/process/release.md](docs/process/release.md) | 发布流程、版本号管理、三平台发布检查清单 |
| [docs/process/test-checklist.md](docs/process/test-checklist.md) | 发布前与关键改动后的回归测试清单 |
| [docs/process/feature-requests.md](docs/process/feature-requests.md) | 功能需求收集、投票口径与评审流程 |
| [docs/process/roadmap.md](docs/process/roadmap.md) | 功能路线图：待实现功能、进度与优先级 |

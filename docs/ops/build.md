# Mocika Shield - 编译指南

## 环境要求

详细环境配置见 [environment.md](environment.md)。

| 工具 | 版本要求 | 说明 |
|------|----------|------|
| Rust | 当前稳定版；桌面共享库声明至少 1.89 | 含 rustup；完整依赖图以锁定构建为准 |
| Java / JDK | 17 | 桌面适配资源构建固定使用完整 JDK 17，且 `java`、`javac`、`keytool` 均需在 PATH；用户运行仍支持 Java 8 |
| Android SDK | Platform 35 | 需设置 `ANDROID_HOME` |
| Android build-tools | 35.0.0 | 提供 `d8` 等 Android 构建工具 |
| Android NDK | 29.0.14206865 + 25.2.9519653 | 标准资源使用 r29；Android 4.4 兼容资源使用 r25c |
| cargo-ndk | 最新 | Android 交叉编译 |
| Python | 3.9+ | 构建期 JNI 映射、混淆规则转换及资源契约审计；非桌面工具运行依赖 |
| Node.js / npm | Node.js 22+ | React 前端构建 |
| tauri-cli | 最新 | GUI 构建驱动（Tauri GUI 需要） |

---

## 快速开始（Makefile）

```bash
make build-stub             # ① 构建 Android 壳（必须最先执行）
make build-cli              # ② 编译 shield 命令行工具
make build-gui              # ③ 构建桌面 GUI Tauri 版（需先 build-stub）
make build-all              # build-stub + build-cli + build-gui（Tauri）
make test                   # 运行 shield-services + shield-core + shield-cli 单元测试
make clean                  # 清理所有构建产物
```

> **构建顺序约束**：`shield`（CLI）和 GUI 运行时均依赖 `resources.zip`，必须先执行 `make build-stub`。

---

## 分步说明

### 1. 构建 shield-stub（Android 壳模块）

```bash
make build-stub
# 等价于：bash scripts/build-stub.sh
```

**Linux / macOS**：运行 `scripts/build-stub.sh`
**Windows**：运行 `scripts\build-stub.ps1`（Makefile 自动选择）

输出：`shield-stub/build/outputs/resources/resources.zip`

壳构建会统一读取 R8 映射、重编译四 ABI Native 并审计最终绑定，原始桥接包名或资源错配会使构建失败。Linux/macOS 默认使用 `python3`，Windows 默认使用 `python`；可通过 `PYTHON` 指定 Python 3.9 以上的可执行文件路径，Gradle 和构建脚本必须使用同一个解释器。

首次执行前需添加 Android Rust 编译目标：

```bash
rustup target add aarch64-linux-android armv7-linux-androideabi \
                  i686-linux-android x86_64-linux-android
```

### 2. 编译 shield-cli

```bash
make build-cli
# 仅编译 Rust 部分：cargo build --release -p shield-cli
```

输出（Linux / macOS）：`target/release/shield`
输出（Windows）：`target\release\shield.exe`

`make build-cli` 会先用构建 JDK 17 生成 Java 8 字节码适配资源，并放到 `target/lib/java-adapter.jar`，供 release 模式的 CLI 相邻定位；普通 `cargo build`／单元检查不会隐式调用 javac。源码调试运行 Windows 工具前可显式执行 `make build-java-adapter`。这是构建要求，用户运行仍支持 Java 8，不需要 javac。

### 3. 构建 shield-gui（Tauri 桌面应用）

```bash
make build-gui
# macOS：bash scripts/build-macos-app.sh
# Linux / Windows：先 python3 scripts/prepare_desktop_cli.py，再进入 GUI 目录：
# cargo tauri build --no-bundle --config ../../target/desktop-cli/tauri-sidecar.json
```

Linux 首次构建前请先安装 Tauri 所需系统依赖，见 [environment.md](environment.md#linux-构建依赖)。

输出（macOS）：`target/release/bundle/macos/MocikaShield.app`，内含独立 `Contents/MacOS/shield`，无需启动 GUI。共包 CLI 启用 `mcp-server`，复用 GUI 配置和证书；MCP 不自启动，默认只读。构建不安装应用或修改 PATH，`make build-cli` 默认不启用这些特性。使用方式见[Agent 接入](agent-setup.md)。

输出（Linux）：`target/release/mocika-shield`
输出（Windows）：`target\\release\\mocika-shield.exe`

Linux／Windows 的 `make build-gui` 用 `scripts/prepare_desktop_cli.py` 准备原生 x64 `mocika-shield-cli`（Windows 带 `.exe`）和生成的 Tauri 配置，材料放在忽略的 `target/desktop-cli/`。普通 Cargo 检查不依赖这些材料。调试 GUI 包须给准备器与 Tauri 同时传 `--debug`；准备器不支持交叉构建或自定义 `CARGO_TARGET_DIR`。

安装时须保留完整应用目录，不只复制可执行文件。三平台命令和资源路径见[共包布局](../design/agent-desktop-platforms.md#桌面共包)，实际覆盖见[平台验收](../process/agent-desktop-platforms-plan.md)。

macOS 共包构建先验证既有标准/工控资源版本，再编译 CLI 和 GUI、核对两者架构与实际资源，最后完成内层 CLI 与整个 `.app` 的 adhoc 签名和校验；不要事后向已经交付的应用复制二进制。`bash scripts/build-macos-app.sh universal` 可构建通用架构，须先安装两个 Apple Rust target；通用架构仍需单独实测。首次构建前在 `apps/shield-gui` 执行 `npm ci`。此入口固定使用仓库 `target`，不接受 `CARGO_TARGET_DIR`。

三平台构建会用 JDK 17 生成并校验共用 `tools/java-adapter.jar`，维护用 CLI 包放在 `lib/`。用户运行仍支持 Java 8，不运行时编译或联网补资源，不将桌面 JAR 放进 Android 壳 ZIP。

Skill 放根目录 `skills/mocika-shield/`，独立安装、升级，不随应用打包；构建拒绝旧 Skill 共包残留。接入配置见[使用说明](agent-setup.md)。

如需生成 `.AppImage`、`.deb`、`.dmg`、NSIS `.exe` 等安装产物，请使用下方发布脚本；macOS 发布脚本复用同一共包入口制作 DMG，Linux／Windows 复用上述准备器与 Tauri 外部二进制配置。`SKIP_CLI_RELEASE=1` 仅跳过独立维护压缩包，不跳过 GUI 内 CLI。

> **本机测试规则**：实际验证 GUI 效果、签名、加固等完整桌面流程须使用应用包。macOS 的 `make build-gui` 现在直接交付 `.app`；其他平台仍不可把裸二进制当作完整安装包。

维护者可对开发分支手动运行 CI 的 `agent_bundles` 范围，生成并验收 Linux deb／AppImage 与 Windows NSIS。它复用已核验的固定 1.4.3 双壳，不重建壳、不改版本或发布；只留存 GUI 安装包，不上传测试 APK 和证书。新版本需先审查固定资源来源，不能在版本不匹配时绕过检查。对已生成并解包／安装的目录可运行 `python3 scripts/verify_desktop_bundle.py <目录> --platform linux`（或 `windows`／`macos`），核对与当前构建的资源及入口；原生及业务实际结果统一见[平台验收](../process/agent-desktop-platforms-plan.md)。

## 发布包构建

### Linux / macOS

```bash
# CLI-only 发布包（维护者本地使用）
make release VERSION=x.y.z

# 仅 CLI 发布包（本地生成，不由 GitHub Release 上传）
bash scripts/release-cli.sh x.y.z

# Linux 本地发布（默认 GUI + CLI；CI 设置 SKIP_CLI_RELEASE=1 只上传 GUI）
VERSION=x.y.z make release-linux

# macOS 本地发布（默认 GUI + CLI；CI 设置 SKIP_CLI_RELEASE=1 只上传 GUI）
VERSION=x.y.z make release-macos
```

### Windows（必须在 Windows 原生环境执行）

```powershell
# Windows 本地发布（默认 GUI + CLI；CI 设置 SKIP_CLI_RELEASE=1 只上传 GUI）
.\scripts\release-windows.ps1 -Version x.y.z
```

---

## 发布包结构

### CLI 本地发布包

```
mocika-shield-x.y.z/
├── bin/
│   └── shield              # 可执行文件（Windows 为 shield.exe）
├── lib/
│   ├── apktool.jar
│   └── apksigner.jar
├── resources/
│   └── resources.zip       # shield-stub 产物
└── README.md
```

### GUI 发布包

| 平台 | 产物 |
|------|------|
| Linux（Tauri） | `MocikaShield_x.y.z_linux_amd64.AppImage`、`MocikaShield_x.y.z_linux_amd64.deb` |
| macOS（Tauri） | `MocikaShield_x.y.z_macos_universal.dmg` |
| Windows | `MocikaShield_x.y.z_windows_x64_setup.exe` |

---

## 常见问题

### cargo-ndk / tauri-cli / npm 未安装

```bash
cargo install cargo-ndk
cargo install tauri-cli --version '^2'
node --version
npm --version
```

Windows 发布脚本会自动检测并安装缺失的 cargo 工具。

### Linux：`failed to run linuxdeploy`

这通常不是代码问题，而是 Linux Tauri / AppImage 打包依赖不完整。

优先检查是否已安装 [environment.md](environment.md#linux-构建依赖) 中列出的系统包，尤其是：

- `file`
- `wget`
- `libxdo-dev`
- `librsvg2-dev`
- `libfuse2`

仓库的完整手动 `CI` 包含 Linux Tauri bundle 冒烟检查；修改 Tauri 打包链路后应主动运行，无需等到打 tag 再排查。

### NDK 未找到

```bash
# 检查已安装版本
ls $ANDROID_HOME/ndk/

# 设置环境变量（优先级高于 build.gradle 内硬编码路径）
export ANDROID_NDK_ROOT=$ANDROID_HOME/ndk/29.0.14206865
```

### Rust 目标未安装

```bash
rustup target add aarch64-linux-android armv7-linux-androideabi \
                  i686-linux-android x86_64-linux-android

# Windows GUI（MSVC 工具链）
rustup target add x86_64-pc-windows-msvc
```

### Gradle 构建失败

```bash
# 清理缓存后重新构建
./shield-stub/gradlew -p shield-stub clean
make build-stub
```

### Windows：`make` 命令不存在

通过 Scoop 安装：

```powershell
scoop install make
```

### Windows：路径问题（UNC 前缀）

VirtualBox 共享文件夹等场景下 `current_exe()` 可能返回 `\\?\UNC\...` 格式路径。
代码已通过 `dunce` crate 自动规范化，无需手动处理。

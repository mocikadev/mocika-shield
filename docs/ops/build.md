# Mocika Shield - 编译指南

## 环境要求

详细环境配置见 [environment.md](environment.md)。

| 工具 | 版本要求 | 说明 |
|------|----------|------|
| Rust | 当前稳定版；桌面共享库声明至少 1.89 | 含 rustup；完整依赖图以锁定构建为准 |
| Java / JDK | 17+ | 必须为完整 JDK，且 `java`、`javac`、`keytool` 均需在 PATH |
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
# 等价于：cargo build --release -p shield-cli
```

输出（Linux / macOS）：`target/release/shield`
输出（Windows）：`target\release\shield.exe`

### 3. 构建 shield-gui（Tauri 桌面应用）

```bash
make build-gui
# macOS：bash scripts/build-macos-app.sh
# Linux / Windows：cd apps/shield-gui && cargo tauri build --no-bundle
```

Linux 首次构建前请先安装 Tauri 所需系统依赖，见 [environment.md](environment.md#linux-构建依赖)。

输出（macOS）：`target/release/bundle/macos/MocikaShield.app`，内含独立 `Contents/MacOS/shield`，不启动 GUI 即可调用。CLI 共用应用内工具与两套资源，显式启用 `mcp-server`（包含原自动化执行特性）；MCP 不自启动，须由宿主显式调用 `mcp serve`，默认只读。不读取 GUI 证书库、不安装到应用目录、不修改系统 PATH。默认 `make build-cli` 仍不启用这些特性，当前公开的 1.4.3 安装包没有这些新入口。

输出（Linux）：`target/release/mocika-shield`
输出（Windows）：`target\\release\\mocika-shield.exe`

macOS 共包构建先验证既有标准/工控资源版本，再编译 CLI 和 GUI、核对两者架构与实际资源，最后完成内层 CLI 与整个 `.app` 的 adhoc 签名和校验；不要事后向已经交付的应用复制二进制。`bash scripts/build-macos-app.sh universal` 可构建通用架构，须先安装两个 Apple Rust target；通用架构仍需单独实测。首次构建前在 `apps/shield-gui` 执行 `npm ci`。此入口固定使用仓库 `target`，不接受 `CARGO_TARGET_DIR`。

Skill 独立放在根目录 `skills/mocika-shield/`，由用户通过所用 Agent 或管理工具安装、升级，不随应用打包；App 构建不依赖该目录。构建会拒绝旧技能资源残留，不把开发期旧共包内容带入最终签名。CLI 仍在 `.app` 内独立运行。安装、路径定位及外部配置说明见 [Agent 接入](agent-setup.md)。

如需生成 `.AppImage`、`.deb`、`.dmg`、NSIS `.exe` 等安装产物，请使用下方发布脚本；macOS 发布脚本复用同一共包入口制作 DMG，其他平台共包尚未接入。

> **本机测试规则**：实际验证 GUI 效果、签名、加固等完整桌面流程须使用应用包。macOS 的 `make build-gui` 现在直接交付 `.app`；其他平台仍不可把裸二进制当作完整安装包。

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

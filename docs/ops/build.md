# Mocika Shield - 编译指南

## 环境要求

详细环境配置见 [environment.md](environment.md)。

| 工具 | 版本要求 | 说明 |
|------|----------|------|
| Rust | 1.70+ | 含 rustup |
| Java / JDK | 8+ | `java`、`javac`、`keytool` 均需在 PATH |
| Android SDK | API 21+ | 需设置 `ANDROID_HOME` |
| Android NDK | 29.0.14206865（硬编码） | 见下方说明 |
| cargo-ndk | 最新 | Android 交叉编译 |
| Node.js / npm | Node.js 22+ | React 前端构建 |
| tauri-cli | 最新 | GUI 构建驱动（Tauri GUI 需要） |

---

## 快速开始（Makefile）

```bash
make build-stub             # ① 构建 Android 壳（必须最先执行）
make build-cli              # ② 编译 shield 命令行工具
make build-gui              # ③ 构建桌面 GUI Tauri 版（需先 build-stub）
make build-all              # build-stub + build-cli + build-gui（Tauri）
make test                   # 运行 shield-cli 单元测试
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

首次执行前需添加 Android Rust 编译目标：

```bash
rustup target add aarch64-linux-android armv7-linux-androideabi \
                  i686-linux-android x86_64-linux-android
```

### 2. 编译 shield-cli

```bash
make build-cli
# 等价于：cargo build --release --manifest-path shield-cli/Cargo.toml
```

输出（Linux / macOS）：`target/release/shield`
输出（Windows）：`target\release\shield.exe`

### 3. 构建 shield-gui（Tauri 桌面应用）

```bash
make build-gui
# 等价于：cd shield-gui && cargo tauri build
```

Linux 首次构建前请先安装 Tauri 所需系统依赖，见 [environment.md](environment.md#linux-构建依赖)。

输出（Linux）：`shield-gui/src-tauri/target/release/bundle/` 下的 AppImage / deb
输出（macOS）：`.dmg` / `.app`
输出（Windows）：NSIS `.exe` / `.msi`

## 发布包构建

### Linux / macOS

```bash
# 旧版 CLI-only 发布包（向后兼容）
make release VERSION=x.y.z

# 仅 CLI 发布包
bash scripts/release-cli.sh x.y.z

# Linux Tauri GUI 全量发布（AppImage + deb + CLI tar.gz）
VERSION=x.y.z make release-linux

# macOS Tauri GUI 全量发布（dmg + CLI tar.gz）
VERSION=x.y.z make release-macos
```

### Windows（必须在 Windows 原生环境执行）

```powershell
# 完整发布包（build-all + NSIS 安装包 + CLI zip）
.\scripts\release-windows.ps1 -Version x.y.z
```

---

## 发布包结构

### CLI 发布包（`mocika-shield-x.y.z.tar.gz`）

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

仓库的普通 `CI` 已新增 Linux Tauri bundle 冒烟检查；如果这里失败，通常不需要等到打 tag 再排查。

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

# 环境要求

## 基础要求

| 工具 | 版本要求 | 说明 |
|------|----------|------|
| Rust | 1.70+ | 含 `rustup` |
| Java / JDK | 17+ | 必须为完整 JDK，且 `java`、`javac`、`keytool` 均在 PATH |
| Android SDK | Platform 35 | 需要 `ANDROID_HOME` 或 `ANDROID_SDK_ROOT` 环境变量 |
| Android NDK | 29.0.14206865 + 25.2.9519653 | 标准模式使用 r29；Android 4.4 兼容模式的 ARMv7 使用 r25c |
| cargo-ndk | 最新 | `cargo install cargo-ndk` |
| Android build-tools | 35.0.0 | 用于 JAR→DEX 转换及发布构建 |
| Tauri CLI | 最新 | 桌面 GUI 构建驱动 |
| Node.js / npm | Node.js 22+ | React 前端构建 |

## Linux 构建依赖

Linux 下构建 Tauri 桌面包时，除 Rust / Node.js / Java / Android SDK 外，还需要系统包：

```bash
sudo apt update
sudo apt install -y \
  build-essential \
  curl \
  wget \
  file \
  libwebkit2gtk-4.1-dev \
  libgtk-3-dev \
  libxdo-dev \
  libayatana-appindicator3-dev \
  librsvg2-dev \
  libssl-dev \
  pkg-config \
  patchelf \
  libfuse2
```

- `file`、`wget`：Tauri / linuxdeploy 在 AppImage 打包流程中会用到
- `libxdo-dev`、`librsvg2-dev`：属于 Tauri 官方 Linux 前置依赖
- `libfuse2`：用于 AppImage 运行时兼容，CI 也保持安装
- 发布构建建议使用 Ubuntu 22.04 或 Debian 12 作为基线，避免把 glibc 要求抬高

## Windows 构建环境

> Windows 发布包（GUI NSIS 安装包 + CLI exe）必须在 Windows 原生环境构建，不支持交叉编译。

### 一键配置（推荐）

已安装 [Scoop](https://scoop.sh) 的情况下，在项目根目录运行：

```powershell
.\scripts\setup-windows-dev.ps1
```

脚本自动完成：Rust 工具链、Java 17、Android SDK/NDK、NSIS、cargo 工具（tauri-cli/cargo-ndk）、全部 Rust target。

> **唯一需要手动安装的**：Visual Studio Build Tools 2022（Scoop 无法静默安装）
> 脚本运行完会给出下载链接和提示。

---

### 手动安装（逐步）

#### 第一步：安装 Scoop

```powershell
Set-ExecutionPolicy RemoteSigned -Scope CurrentUser
irm get.scoop.sh | iex
```

#### 第二步：通过 Scoop 安装工具

```powershell
# 添加 bucket
scoop bucket add java
scoop bucket add extras

# 安装基础工具
scoop install git make 7zip rustup nsis

# Java（shield-stub 构建需要）
scoop install temurin17-jdk

# Android 命令行工具（含 sdkmanager，用于安装 SDK / NDK）
scoop install android-clt
```

#### 第三步：配置 Rust 工具链

```powershell
# 设置为 MSVC toolchain（Tauri 要求）
rustup default stable-msvc

# Rust targets
rustup target add x86_64-pc-windows-msvc   # Windows CLI/GUI
rustup target add aarch64-linux-android armv7-linux-androideabi i686-linux-android x86_64-linux-android  # Android 壳
rustup toolchain install 1.77.2 --profile minimal --target armv7-linux-androideabi  # Android 4.4 兼容壳
```

#### 第四步：安装 cargo 工具

```powershell
cargo install cargo-ndk    # Android 交叉编译
cargo install tauri-cli    # GUI 构建驱动
```

#### 第五步：安装 Android NDK

```powershell
# 设置 ANDROID_HOME（android-clt 的 scoop 安装路径）
$env:ANDROID_HOME = "$env:USERPROFILE\scoop\apps\android-clt\current"
[System.Environment]::SetEnvironmentVariable("ANDROID_HOME", $env:ANDROID_HOME, "User")

# 通过 sdkmanager 安装指定版本 NDK
"y" | sdkmanager "ndk;29.0.14206865" "ndk;25.2.9519653"
```

#### 第六步：安装 Visual Studio Build Tools 2022（必须手动）

下载：<https://visualstudio.microsoft.com/visual-cpp-build-tools/>

安装时勾选「**使用 C++ 的桌面开发**」（含 MSVC 编译器和 Windows SDK）。

---

### 工具可用性一览

| 工具 | Scoop 安装 | 备注 |
|------|-----------|------|
| Rust / rustup | `scoop install rustup` | main bucket |
| Java 17 | `scoop install temurin17-jdk` | java bucket |
| Android SDK | `scoop install android-clt` | main bucket，含 sdkmanager |
| Android NDK | `sdkmanager "ndk;29.0.14206865"` | 通过 android-clt 的 sdkmanager |
| NSIS | `scoop install nsis` | extras bucket |
| make | `scoop install make` | main bucket |
| cargo-ndk | `cargo install cargo-ndk` | Scoop 无此包 |
| tauri-cli | `cargo install tauri-cli` | Scoop 无此包 |
| Node.js / npm | `scoop install nodejs-lts` | React 前端构建 |
| MSVC Build Tools | 手动安装 | Scoop 无法静默安装 |

### 运行发布脚本

```powershell
# 在项目根目录执行（环境配置完成后）
.\scripts\release-windows.ps1

# 指定版本号
.\scripts\release-windows.ps1 -Version 1.2.3
```

发布脚本会再次检测所有依赖，缺失的 cargo 工具（tauri-cli/cargo-ndk）会自动安装。

---

## NDK 版本配置

`shield-stub/build.gradle.kts` 中 `buildRustLibs` 任务**硬编码**了 NDK 路径：

```kotlin
// 优先级：ANDROID_HOME/ndk/29.0.14206865 > ANDROID_NDK_ROOT > NDK_HOME
val ndkRoot = System.getenv("ANDROID_HOME")
    ?.let { "$it/ndk/29.0.14206865" }
    ?.takeIf { file(it).isDirectory }
    ?: System.getenv("ANDROID_NDK_ROOT")
    ?: System.getenv("NDK_HOME")
    ?: error("未设置 ANDROID_NDK_ROOT、NDK_HOME 或 ANDROID_HOME，无法定位 NDK")
environment("ANDROID_NDK_ROOT", ndkRoot)
```

若本地 NDK 版本不同，有两种解决方式：

1. **修改 build.gradle**：将版本号改为本地已安装的 NDK 版本
2. **设置环境变量**：`export ANDROID_NDK_ROOT=/path/to/your/ndk`（`build.sh` 优先读取此变量）

## Android 目标架构安装

首次交叉编译 `shield-stub` 前，必须通过 `rustup` 添加 Android 目标架构：

```bash
rustup target add aarch64-linux-android armv7-linux-androideabi \
                  i686-linux-android x86_64-linux-android
```

## 仅使用发布包

如果只是使用发布包（不从源码编译），仅需：

- Linux / macOS / Windows 系统
- Java 17+（需完整 JDK，`java` / `javac` / `keytool` 在 PATH）

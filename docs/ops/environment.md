# 环境要求

## 使用桌面安装包

安装完整 JDK 8 或以上版本，确保应用及 Agent 宿主能找到 `java`、`keytool`；不需要 Rust、Android SDK、NDK、Node.js 或 Python。

Windows 要求 Windows 10 及以上 64 位系统。macOS 安装未公证提示和 Linux 运行问题见[README](../../README.md#下载与快速开始)与[排障](troubleshooting.md)。

Java 优先从 PATH 查找，再查 `JAVA_HOME/bin`；keytool 优先使用所选 Java 对应的 JDK，兼容 Java 8 的 `jre/` 布局，再按环境回退。缺 keytool 时确认安装的是完整 JDK，`JAVA_HOME` 应指向 JDK 根目录而不是 `bin`。

修改环境变量后重启应用及相关 Agent 宿主；“刷新环境”只能检查当前进程已继承的环境，不会重新继承系统 PATH。先在相同环境验证：

```bash
java -version
keytool -help
```

纯加固和使用已保存证书独立签名不额外要求 keytool；导入／创建／校验证书及加固后自动签名前的指纹核对仍需要它，因此推荐完整 JDK。该依赖校准已随 1.4.3 发布，不保证任意 JRE 与证书组合都可用。

## 源码构建

| 工具 | 要求 |
|---|---|
| Rust | 当前稳定工具链；共享桌面库至少 1.89，完整依赖以锁定构建为准 |
| 构建 JDK | 完整 JDK 17，`java`、`javac`、`keytool` 可用 |
| Android SDK | Platform 35、build-tools 35.0.0，设置 `ANDROID_HOME` |
| 标准 NDK | r29：`29.0.14206865` |
| 工控兼容工具链 | r25c：`25.2.9519653`、Rust 1.77.2、API 19 |
| Python | 3.9+，用于构建期映射、契约生成和审计 |
| Node.js／npm | Node.js 22+ |
| Cargo 工具 | `cargo-ndk`、Tauri CLI 2.x |

构建 JDK 17 与用户运行 Java 8 是两套要求；API 19 独立工具链不随桌面 Rust 下限升级。

```bash
cargo install cargo-ndk --locked
cargo install tauri-cli --version '^2' --locked
rustup target add aarch64-linux-android armv7-linux-androideabi i686-linux-android x86_64-linux-android
rustup toolchain install 1.77.2 --profile minimal --target armv7-linux-androideabi
```

安装双 NDK，不用降低标准构建的 NDK 来兼容 4.4：

```bash
sdkmanager "platforms;android-35" "build-tools;35.0.0" "ndk;29.0.14206865" "ndk;25.2.9519653"
```

`shield-stub/build.gradle.kts` 优先使用存在的 `ANDROID_HOME/ndk/29.0.14206865`，再看 `ANDROID_NDK_ROOT`、`NDK_HOME`。不要误以为环境变量一定能覆盖已安装的固定版本，也不要为本机方便随意换 NDK；工具链升级须另行验证。详细构建见[指南](build.md)。

## Linux 构建依赖

正式 Linux 发布使用 Ubuntu 22.04。Debian／Ubuntu 系统包示例：

```bash
sudo apt update
sudo apt install -y build-essential curl wget file libwebkit2gtk-4.1-dev libgtk-3-dev libxdo-dev libayatana-appindicator3-dev librsvg2-dev libssl-dev pkg-config patchelf libfuse2
```

WebKitGTK／GTK 用于 GUI；`file`、`wget`、`patchelf` 等参与打包，`libfuse2` 用于 AppImage 兼容。其他发行版包名不同，不把 Ubuntu 构建结果当作 Fedora 渲染实测。

## Windows 构建环境

GUI／NSIS 必须在 Windows 原生环境构建，不从 macOS／Linux 交叉打包。已安装 Scoop 时可在仓库根目录运行：

```powershell
.\scripts\setup-windows-dev.ps1
```

该脚本会安装开发依赖并配置环境；请先检查脚本，勿将其作为普通用户运行 App 的前置步骤。Visual Studio Build Tools 2022 须另行安装并选择“使用 C++ 的桌面开发”（含 MSVC 和 Windows SDK）。

手动配置需准备 Git、Make、7-Zip、Python、Node.js、Rust MSVC、JDK 17、Android 命令行工具、双 NDK、NSIS 和上述 Cargo 工具。4.4 壳的现有构建入口还使用 Git Bash；这不表示 Windows 用户运行 CLI／Skill／MCP 需要 Bash。

环境就绪后按[构建指南](build.md)执行，发布规则见[发布流程](../process/release.md)。不在多个文档重复维护整套安装步骤。

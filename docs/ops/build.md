# 源码构建指南

使用安装包不需要 Rust、NDK 或 Node.js，直接看[使用指南](../usage.md)。源码环境及平台依赖见[环境要求](environment.md)。

## 构建顺序

在项目根目录执行：

```bash
make build-stub   # 标准、Android 4.4 工控及内部候选壳资源
make build-cli    # 普通 CLI
make build-gui    # GUI 与包含 Agent／MCP 的共包 CLI
make test        # 共享服务、核心与普通 CLI 单元测试
```

也可用 `make build-all` 按顺序构建。CLI／GUI 运行时依赖壳 ZIP；仅编译 Rust 通过不代表资源已就绪。构建和打包必须使用同版本标准／工控资源，禁止只改元数据版本绕过检查。

## 壳资源

`make build-stub` 调用当前系统的标准构建入口，再生成 Android 4.4 兼容资源：

- 标准四 ABI：NDK r29 `29.0.14206865`。
- 工控 ARMv7：NDK r25c `25.2.9519653`、Rust `1.77.2`、API 19；其他主流 ABI 沿用标准库。
- 输出目录：`shield-stub/build/outputs/resources/`，正式桌面包使用 `resources.zip` 和 `resources-api19.zip`。
- `resources-memory.zip` 只供内部实验，不进入正式桌面产品能力。

构建读取同批 R8 映射，联动编译 Native 并审计 JNI、Manifest 契约及导出面。Linux／macOS 默认用 `python3`，Windows 默认用 `python`；可设置 `PYTHON`，Gradle 和脚本须指向同一解释器。

## 普通 CLI 与 Agent 特性

```bash
make build-cli
# 需要 Agent／MCP 的源码调试入口：
make build-java-adapter
cargo build --locked -p shield-cli --features mcp-server
```

普通 `make build-cli` 不启用受控执行／MCP；`mcp-server` 包含 `agent-executor`。输出为 `target/release/shield`（Windows 带 `.exe`）；未传 `--release` 的 Cargo 命令输出到 `target/debug/`。

Java 适配资源固定用完整 JDK 17 编译为 Java 8 字节码；`make build-cli` 将其放入 `target/lib/java-adapter.jar`。单独 Cargo 检查不会隐式调用 javac。用户运行仍支持 Java 8，不运行时编译或联网补 JAR。

## 桌面共包

先在 `apps/shield-gui` 执行一次 `npm ci`，再返回根目录执行 `make build-gui`。

| 平台 | 构建方式与输出 |
|---|---|
| macOS | `scripts/build-macos-app.sh` 生成 `target/release/bundle/macos/MocikaShield.app` |
| Linux／Windows | `scripts/prepare_desktop_cli.py` 准备原生 x64 CLI 和配置，随后 Tauri `--no-bundle` 编译 GUI；可执行文件位于 `target/release/`，不是完整安装包 |

macOS 入口核对版本、GUI／CLI 架构、JAR 和双壳，最后对内层 CLI 和完整应用做 adhoc 签名。通用架构用 `bash scripts/build-macos-app.sh universal`，需两个 Apple Rust target。

Linux／Windows 准备器生成忽略提交的 `target/desktop-cli/` 和 `tauri-sidecar.json`，不支持交叉构建；调试时准备器和 Tauri 都须传 `--debug`。这些共包入口固定使用仓库 `target`，不能设置 `CARGO_TARGET_DIR`。

本机 GUI 测试须交付完整应用；macOS 不只运行裸二进制。Linux／Windows 分发须用安装包并保留资源目录。三平台实际路径、Skill 安装和 MCP 配置见[Agent 接入](agent-setup.md)。

## 安装包构建

```bash
VERSION=x.y.z make release-linux
VERSION=x.y.z make release-macos
# macOS Universal
VERSION=x.y.z make release-macos-universal
```

Windows 须在原生环境运行：

```powershell
.\scripts\release-windows.ps1 -Version x.y.z
```

产物位于 `dist/<平台>/`。本地脚本默认还生成维护用 CLI 压缩包；`SKIP_CLI_RELEASE=1` 只跳过独立压缩包，不省略 GUI 包内 CLI。Skill 从 `skills/mocika-shield/` 独立安装，不参与 App 打包。

只需维护用 CLI 压缩包时可执行 `bash scripts/release-cli.sh x.y.z`；GitHub Release 不上传它。产物命名、校验和及发布权限见[发布流程](../process/release.md)。

## 验证与排障

- 快速检查：`cargo fmt --all --check`、`python3 -m unittest discover -s scripts/tests -q`。
- 实际安装包核对：`python3 scripts/verify_desktop_bundle.py <解包或安装目录> --platform linux`；平台也可为 `windows`／`macos`，使用同次构建的源码和资源。
- 修改共包链路后可手动运行 `agent_bundles`，修改原生执行／Java 层按[平台验收分工](../process/agent-desktop-platforms-plan.md)选择范围；纯文档修改不重跑重型构建。
- Linux 打包缺依赖先查[系统包](environment.md#linux-构建依赖)；其他问题见[排障](troubleshooting.md)。

`make clean` 会删除构建产物、资源和前端依赖；仅在确需全量重建时使用，不作为日常排障的第一步。

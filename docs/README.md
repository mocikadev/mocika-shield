# Mocika Shield 文档总览

本文档目录面向维护者和发布人员，用于快速定位构建、使用、设计和流程资料。用户快速开始优先阅读根目录 [README.md](../README.md)。

## 当前工程概览

Mocika Shield 是 Android APK 加固工具，核心流程是将原 APK 的 DEX 压缩加密后追加到 `classes.dex` 末尾，并由 Android 壳模块在运行时完成签名校验、解密和 DEX 注入。

仓库内主要模块：

| 模块 | 说明 |
|------|------|
| `crates/shield-core/` | Rust 共享核心库，承载 APK 加固、签名、ZIP 对齐、Java/工具探测等共用能力 |
| `crates/shield-app/` | 多入口共享应用库，承载证书、目录、配置和身份；GUI／CLI 共用，MCP 通过 CLI 在白名单内使用 |
| `apps/shield-cli/` | Rust CLI，负责参数解析、终端输出与 JSON 进度协议 |
| `shield-stub/` | Android 壳模块，包含 Java 壳层和 Rust JNI native 库 |
| `apps/shield-gui/` | Tauri v2 + React 桌面 GUI，唯一正式 GUI，目标覆盖 Linux / macOS / Windows |
| `skills/` | 面向使用者 Agent 的独立技能源码，由用户安装和升级，不随 App 打包，不承载核心执行实现 |
| `scripts/` | 构建、版本同步、发布脚本 |
| `tools/` | `apktool`、`apksigner` 等外部工具 |

## 阅读路线

| 场景 | 建议阅读 |
|------|----------|
| 第一次使用 | [../README.md](../README.md)、[usage.md](usage.md) |
| 配置开发环境 | [ops/environment.md](ops/environment.md) |
| 从源码构建 | [ops/build.md](ops/build.md) |
| 安装本地 Agent Skill、配置共包 CLI 与验收客户端 | [ops/agent-setup.md](ops/agent-setup.md) |
| 本地排障 | [ops/troubleshooting.md](ops/troubleshooting.md) |
| 查看维护统计方案 | [ops/project-statistics.md](ops/project-statistics.md) |
| 查看匿名使用统计方案 | [ops/telemetry.md](ops/telemetry.md) |
| 查询国家／地区分布、未知覆盖率及服务端部署 | [ops/usage-country-statistics.md](ops/usage-country-statistics.md) |
| 查看匿名失败分类与确认发送报告方案 | [design/failure-diagnostics.md](design/failure-diagnostics.md) |
| 查看错误报告有限证据、环境补齐与旧协议兼容 | [design/error-report-evidence.md](design/error-report-evidence.md) |
| 部署失败诊断、按版本查询及回滚 | [ops/failure-diagnostics.md](ops/failure-diagnostics.md) |
| 按应用记忆选择的加固与签名使用情况分享（Beta.6 起） | [design/application-usage-sharing.md](design/application-usage-sharing.md)、[实施计划](superpowers/plans/2026-09-18-application-usage-sharing.md)、[运维步骤](ops/application-usage-sharing.md) |
| 理解加固实现 | [design/internals.md](design/internals.md)、[design/architecture.md](design/architecture.md) |
| 了解 Native 库打包、`extractNativeLibs` 与 16 KB 兼容设计 | [design/native-library-packaging.md](design/native-library-packaging.md) |
| 了解旧 SDK 附带不支持 ABI 时的确认与排除策略 | [design/unsupported-abi.md](design/unsupported-abi.md) |
| 了解 Native 库名去品牌化与按任务别名方案 | [design/native-library-alias.md](design/native-library-alias.md) |
| 了解壳包名残留、R8 混淆与 JNI 绑定治理方案 | [design/stub-identity.md](design/stub-identity.md) |
| 了解 Native 最小导出、调试符号与构建路径检查 | [design/native-surface.md](design/native-surface.md) |
| 了解 Manifest 中性字段、资源协议与旧资源兼容 | [design/manifest-runtime-contract.md](design/manifest-runtime-contract.md) |
| 了解 4.4 ARM 与新系统混合 ABI 单包兼容任务 | [design/android-4.4-mixed-abi.md](design/android-4.4-mixed-abi.md) |
| 了解 Stub DEX 最小化、能力变体与二阶段加载实验 | [design/stub-dex-minimization.md](design/stub-dex-minimization.md) |
| 了解运行时安全、缓存认证与 Root 策略规划 | [design/runtime-security.md](design/runtime-security.md) |
| 了解 API 31 以上内存 DEX 正式接入边界 | [design/memory-dex-production.md](design/memory-dex-production.md) |
| 了解 DEX 结构与方法代码分离研究 | [design/dex-code-separation.md](design/dex-code-separation.md) |
| 了解工具链轻量化、CI 与 Gradle 集成规划 | [design/toolchain-modernization.md](design/toolchain-modernization.md) |
| 了解 Agent、Skill、本地 MCP 接入与安全边界 | [design/agent-integration.md](design/agent-integration.md)、[阶段任务与验收记录](process/agent-integration-plan.md) |
| 了解 GUI／CLI／MCP／未来 TUI 共用配置、证书与业务能力的调整 | [design/shared-application.md](design/shared-application.md)、[实施计划](process/shared-application-plan.md) |
| 了解受控共享身份、记录级绑定及跨调用无状态传输设计 | [design/shared-agent-binding.md](design/shared-agent-binding.md) |
| 了解 MCP 共享模式、证书白名单和会话私有绑定 | [design/shared-mcp.md](design/shared-mcp.md) |
| 了解 MCP 共享偏好保存、证书编辑与默认项管理 | [design/shared-mcp-management.md](design/shared-mcp-management.md)、[实施计划](process/shared-mcp-management-plan.md) |
| 了解受控独立签名、已加固包绑定检查与 MCP 无人值守单次调用方案 | [design/controlled-signing.md](design/controlled-signing.md)、[实施计划](process/controlled-signing-plan.md) |
| 使用开发分支 CLI 管理共享资料、独立签名与加固后签名 | [ops/shared-data-cli.md](ops/shared-data-cli.md) |
| 了解 Agent 工作子进程、取消与原子无覆盖提交 | [design/agent-executor.md](design/agent-executor.md) |
| 了解自动化命令、结果协议与取消/断开语义 | [design/agent-cli-contract.md](design/agent-cli-contract.md) |
| 了解 macOS 本地 MCP、启动权限与实施任务 | [design/local-mcp.md](design/local-mcp.md)、[process/local-mcp-plan.md](process/local-mcp-plan.md) |
| 查看中文路径、Java 工具输出编码修复与验证边界 | [design/chinese-toolchain-compatibility.md](design/chinese-toolchain-compatibility.md) |
| 了解可选资源文件保护边界 | [design/asset-protection.md](design/asset-protection.md) |
| 了解 Android 4.4 工控兼容方案 | [design/android-4.4-compatibility.md](design/android-4.4-compatibility.md) |
| 查看 AAB 加固可行性结论与后续规划 | [design/aab-feasibility.md](design/aab-feasibility.md) |
| 维护 GUI | [design/gui.md](design/gui.md) |
| 排查中文证书 Alias 与 Java 输出编码 | [中文 Alias 修复计划与回归记录](superpowers/plans/2026-09-16-keytool-chinese-alias.md) |
| 规划目录重构 | [design/refactor-plan.md](design/refactor-plan.md) |
| 发布新版本 | [process/release.md](process/release.md) |
| 管理分支与 PR | [process/release.md](process/release.md#main-分支保护规则) |
| 发布前回归 | [process/test-checklist.md](process/test-checklist.md) |
| 反馈问题 | [process/support.md](process/support.md) |
| 提交或评审功能建议 | [process/feature-requests.md](process/feature-requests.md) |
| 提交代码 | [process/commit-convention.md](process/commit-convention.md) |
| 查看后续计划 | [process/roadmap.md](process/roadmap.md) |
| 追溯已完成版本与旧研究过程 | [process/roadmap-history.md](process/roadmap-history.md) |

## 文档归类

| 目录 | 放置内容 |
|------|----------|
| `ops/` | 环境配置、构建、本地排障等操作手册 |
| `design/` | 架构、内部格式、GUI 设计与维护约束 |
| `process/` | 提交规范、版本管理、发布流程、问题反馈、路线图 |

## 维护规则

- 根目录 `README.md` 保持用户视角，只放快速开始、功能概览和最少量原理说明。
- `AGENTS.md` 只保留项目专属高优先级约束和文档导航，详细流程放入 `docs/`。
- 涉及构建命令、发布产物命名、版本同步规则时，同时核对 `Makefile` 和 `scripts/`。
- 文档中描述路线图功能时直接写功能名称，不使用内部追踪编号。

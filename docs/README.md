# Mocika Shield 文档总览

本文档目录面向维护者和发布人员，用于快速定位构建、使用、设计和流程资料。用户快速开始优先阅读根目录 [README.md](../README.md)。

## 当前工程概览

Mocika Shield 是 Android APK 加固工具，核心流程是将原 APK 的 DEX 压缩加密后追加到 `classes.dex` 末尾，并由 Android 壳模块在运行时完成签名校验、解密和 DEX 注入。

仓库内主要模块：

| 模块 | 说明 |
|------|------|
| `shield-cli/` | Rust CLI，提供 APK 加固、签名能力，也作为库被 Tauri GUI 直接链接 |
| `shield-stub/` | Android 壳模块，包含 Java 壳层和 Rust JNI native 库 |
| `shield-gui/` | Tauri v2 + React 桌面 GUI，唯一正式 GUI，目标覆盖 Linux / macOS / Windows |
| `scripts/` | 构建、版本同步、发布脚本 |
| `tools/` | `apktool`、`apksigner` 等外部工具 |

## 阅读路线

| 场景 | 建议阅读 |
|------|----------|
| 第一次使用 | [../README.md](../README.md)、[usage.md](usage.md) |
| 配置开发环境 | [ops/environment.md](ops/environment.md) |
| 从源码构建 | [ops/build.md](ops/build.md) |
| 理解加固实现 | [design/internals.md](design/internals.md)、[design/architecture.md](design/architecture.md) |
| 维护 GUI | [design/gui.md](design/gui.md) |
| 规划目录重构 | [design/refactor-plan.md](design/refactor-plan.md) |
| 发布新版本 | [process/release.md](process/release.md) |
| 提交代码 | [process/commit-convention.md](process/commit-convention.md) |
| 查看后续计划 | [process/roadmap.md](process/roadmap.md) |

## 文档归类

| 目录 | 放置内容 |
|------|----------|
| `ops/` | 环境配置、构建、故障排查等操作手册 |
| `design/` | 架构、内部格式、GUI 设计与维护约束 |
| `process/` | 提交规范、版本管理、发布流程、路线图 |

## 维护规则

- 根目录 `README.md` 保持用户视角，只放快速开始、功能概览和最少量原理说明。
- `AGENTS.md` 只保留项目专属高优先级约束和文档导航，详细流程放入 `docs/`。
- 涉及构建命令、发布产物命名、版本同步规则时，同时核对 `Makefile` 和 `scripts/`。
- 文档中描述路线图功能时直接写功能名称，不使用内部追踪编号。

# 文档导航

第一次使用先看[项目 README](../README.md)。文档按“如何使用”“如何维护”“为什么这样设计”分开；历史计划不作为当前操作步骤。

## 使用与接入

| 需要做什么 | 文档 |
|---|---|
| 加固、签名、管理证书 | [使用指南](usage.md) |
| 配置 Java、排查安装／启动／签名问题 | [环境要求](ops/environment.md)、[本地排障](ops/troubleshooting.md) |
| 接入 CLI、Skill 或 MCP | [三平台 Agent 接入](ops/agent-setup.md)、[共享资料命令](ops/shared-data-cli.md) |
| 了解发送了哪些数据、如何关闭 | [统计、错误报告与应用分享](ops/telemetry.md) |
| 提交问题、样本或功能建议 | [反馈指南](process/support.md)、[功能需求](process/feature-requests.md) |

## 开发与维护

| 需要做什么 | 文档 |
|---|---|
| 从源码构建 | [构建指南](ops/build.md)、[工程架构](design/architecture.md) |
| 提交、合并和发布 | [提交规范](process/commit-convention.md)、[发布流程](process/release.md) |
| 选择验证范围 | [测试清单](process/test-checklist.md)、[Agent 平台验收](process/agent-desktop-platforms-plan.md) |
| 查看当前任务／历史决定 | [路线图](process/roadmap.md)、[历史档案](process/roadmap-history.md) |
| 查询下载、关注及匿名使用 | [项目统计](ops/project-statistics.md)、[地区统计](ops/usage-country-statistics.md) |
| 维护错误报告／应用分享 | [失败诊断运维](ops/failure-diagnostics.md)、[应用分享运维](ops/application-usage-sharing.md) |

## 技术设计

按需要阅读，不要求从头遍历全部设计文档：

- **加固与兼容**：[技术内参](design/internals.md)、[Native 打包与 16 KB](design/native-library-packaging.md)、[不支持的 ABI](design/unsupported-abi.md)、[Android 4.4](design/android-4.4-compatibility.md)、[混合 ABI](design/android-4.4-mixed-abi.md)。
- **运行时与壳特征**：[缓存／Root 策略](design/runtime-security.md)、[Native 库别名](design/native-library-alias.md)、[壳身份与 JNI](design/stub-identity.md)、[导出面](design/native-surface.md)、[Manifest 契约](design/manifest-runtime-contract.md)、[Stub DEX 精简](design/stub-dex-minimization.md)。
- **桌面与工具链**：[GUI](design/gui.md)、[Java 探测](design/java-tool-discovery.md)、[中文兼容](design/chinese-toolchain-compatibility.md)、[Java 参数适配](design/java-tool-adapter.md)、[处理成本基线](design/apk-processing-baseline.md)、[轻量化边界](design/toolchain-modernization.md)。
- **Agent 与共享服务**：[整体接入](design/agent-integration.md)、[共享应用](design/shared-application.md)、[受控执行器](design/agent-executor.md)、[CLI 契约](design/agent-cli-contract.md)、[共享绑定](design/shared-agent-binding.md)、[三平台适配](design/agent-desktop-platforms.md)。
- **MCP**：[传输与会话](design/local-mcp.md)、[共享范围](design/shared-mcp.md)、[单次签名](design/controlled-signing.md)、[管理](design/shared-mcp-management.md)、[证书生命周期](design/mcp-certificate-lifecycle.md)。
- **数据通道**：[统计构建身份](design/telemetry-build-identity.md)、[失败诊断](design/failure-diagnostics.md)、[有限错误证据](design/error-report-evidence.md)、[应用分享](design/application-usage-sharing.md)。

## 研究与历史记录

以下不是已支持功能或自动恢复的待办：

- [内存 DEX 候选](design/memory-dex-production.md)：长期内存门槛未过，暂停产品接入。
- [方法代码分离](design/dex-code-separation.md)：离线原型完成，运行时方案停止。
- [AAB](design/aab-feasibility.md)、[可选资源保护](design/asset-protection.md)：候选方向，进入条件见路线图。
- [早期模块拆分](design/refactor-plan.md)：已完成的重构结论，不是新的目录迁移任务。
- `process/` 中的版本／阶段记录和 `superpowers/` 中的历史方案用于追溯；最新能力看接入说明，最新证据看专项验收，不执行旧记录中的迁移或发布步骤。

## 文档维护规则

| 位置 | 只维护什么 |
|---|---|
| 根 README | 用户概览、下载、最短开始步骤 |
| `usage.md`、`ops/` | 当前可执行操作、排障、环境和运维 |
| `design/` | 长期有效的契约、边界、取舍和研究结论 |
| `process/` | 发布／测试规则、当前计划与必要验收证据 |
| `AGENTS.md` | 项目专属高优先级约束和入口，不复制全部手册 |
| `skills/mocika-shield/` | 独立安装后仍可使用的 Agent 操作指引 |

当前规则只保留一份；跨文档用链接引用，不重复粘贴整套命令和能力清单。完成的阶段记录结果、证据入口和未验范围，不积累每轮测试总数、临时绝对路径或等待状态。历史记录标明时间，不保留过期“下一步”。

修改命令、路径、版本或发布产物时核对源码／脚本；保留必要的隐私与失败边界。不能为了精简把“未验证”改为“支持”，也不为文档收口重复完整设备矩阵。

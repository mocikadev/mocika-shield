# Agent 三平台任务与验收

更新：2026-11-11。设计见[三平台适配](../design/agent-desktop-platforms.md)，工具细项见[Java 工具验收](java-tool-adapter-plan.md)。当前稳定版仍为 1.4.3；功能已通过集中 PR 合并，发布目标为 1.5.0-beta.1。开发验收不替代新版本正式产物核对。

## 任务状态

- [x] Linux／Windows 原生执行、文件身份、私有材料、取消／标准流及无覆盖提交接线。
- [x] Windows Java 参数适配、共享证书和普通签名／加固入口。
- [x] 三平台 GUI 共包 CLI、辅助 JAR 与双壳，Skill 独立分发。
- [x] macOS 本机完整 `.app`、Java 8／17 包级与既有行为回归。
- [x] Linux deb／AppImage、Windows NSIS 的实际安装布局及业务闭环。
- [x] macOS／Linux 最新源码签名、协议、共享资料及故障清场专项。
- [x] Windows 源码专项的独立 SDK 对齐断言复验。
- [x] 精简本阶段文档与开发提交。
- [ ] 完成集中 PR 审查。
- [ ] 合并后由维护者决定预览版本与发布，不自动移动标签。

## 当前证据

| 范围 | 结果 | 依据 |
|------|------|------|
| macOS arm64 `.app`，Java 8／17 | 通过 | 本机完整构建／严格签名，源码外 CLI／MCP、双模式加固签名、取消／断开及包不变核对 |
| Linux x64 deb／AppImage，Java 17 | 通过 | [实际产物专项](https://github.com/mocikadev/mocika-shield/actions/runs/43000056366) |
| Windows x64 NSIS，Java 17 | 通过 | 同一产物专项，隔离运行机中文空格安装目录 |
| 三平台源码专项，Java 17 | 全部通过 | [最终原生专项](https://github.com/mocikadev/mocika-shield/actions/runs/43006447396) |
| Windows 普通工具，Java 8／17 | 通过 | [普通工具专项](https://github.com/mocikadev/mocika-shield/actions/runs/42992520650) |

独立原工具验签及 SDK 对齐使用与中文产物逐字节相同的 ASCII 副本，避免原工具 argv 的路径损坏污染结论；被测签名、加固、指纹和中文输入／输出仍使用原路径，不放宽验签或对齐要求。

早期失败记录保留在 Git／CI 历史，不作为通过证据。额外系统、JDK、CPU 架构、宿主原生注册／按钮、GUI 人工交互与 Android 设备业务矩阵未全验，不外推承诺，也不作为本阶段一致的人工前置条件。

## 手动验证分工

| CI 范围 | 验证内容 | 不做什么 |
|---------|----------|----------|
| `agent_platforms` | 三平台源码、原生组件、布局、协议、共享资料、真实签名及清场 | 不生成完整加固夹具，不扫描要求双壳／DEX 的加固测试，不构建 GUI／壳 |
| `agent_bundles` | 与正式发布一致的 Ubuntu 22.04／Windows 2025，实际 GUI 包及包内 CLI／MCP | 不重新构建壳、不发布、不生成独立 CLI 资产 |
| `agent_processes`／`agent_windows_processes` | 进程、文件和标准流底层修改的定向验证 | 不把组件通过当作产品通过 |
| `agent_java` | 工具身份及 Java 子进程目录 | 不替代完整证书或 APK 业务验证 |

安装包专项复用固定摘要的公开 1.4.3 双壳；样本从仓库 `agent-protection`／`agent-signing` 源码构建，只用临时证书和隔离 `--app-home`。每个平台一次公开资源下载算维护下载。后续版本须同步审查固定来源，不绕过版本／摘要检查。

Linux 分别解包 deb／AppImage，Windows 在一次性 CI 运行机静默安装 NSIS，不打开 GUI。`verify_desktop_bundle.py` 核对五份资源摘要、版本、命令／依赖、Skill 不共包及包不变；真实业务测试不提供项目工具路径覆盖，验证证书创建校验、普通签名、标准／工控加固签名、MCP 15 项工具发现及共享签名，并用原工具／SDK 核验。

只留存 GUI 安装包七天，不上传裸 CLI、APK、证书、资料库或日志目录。重任务仅手动运行，按代码影响选择范围；文档或提交历史整理不重复安装包构建。

## 收口条件

1. 三平台原生专项与实际产品验收通过，后续仅回填文档，不重复完整测试。
2. 文档、目录和产物清单一致，开发提交压缩后只形成一个完整阶段 PR。
3. 不在收口中追加功能或扩张已验证范围；合并、版本及公开发布分别决定。

# Agent 三平台适配

GUI、CLI、Skill、MCP 共享本地业务能力；本次补齐 Linux／Windows 执行与桌面共包，不增加后台服务或第二套任务状态机。状态见[验收表](../process/agent-desktop-platforms-plan.md)，用法见[Agent 接入](../ops/agent-setup.md)。

## 架构边界

- GUI／CLI 依赖 `shield-services` 和 `shield-core`；MCP 通过同一 CLI 调用，不要求 GUI 常驻。
- CLI 父进程拥有任务、总期限、停止确认及最终提交；工作进程只处理私有候选。
- core 负责加固、签名、证书工具和 Java 适配，不依赖 CLI、Tauri、Tokio 或数据库。
- 保留公开计划、共享绑定、V1–V3 受控范围、输出不覆盖及未知不重试规则。
- 普通源码 CLI 默认不启用执行器／MCP；桌面共包显式启用 `mcp-server`。Skill 独立安装、升级。

## 原生执行与文件规则

| 边界 | macOS／Linux | Windows |
|------|--------------|---------|
| 进程及后代 | 保留组长，确认进程组停止后回收；Linux 有界读取 `/proc`，僵尸不误判运行 | 本次 Job Object，确认全部活跃成员归零及直属句柄退出，不按进程名杀进程 |
| 开始／父死 | 控制管道租约，后代不继承 | 外层 Job 先确认工作端归属，再发送开始帧；工作端收到开始后建立内层 Job；父死关闭外层句柄兜底 |
| 普通命令启动 | 复用进程组 | 通过 `CreateProcessW` 的 Job 属性在创建时归属，不在启动后补分配 |
| 文件身份 | 设备／inode、所有者、权限与句柄核对 | 卷／128 位文件 ID、重解析点及 ACL 核对，不伪造 Unix 字段 |
| 最终提交 | 固定目录句柄，无覆盖原子链接 | 相对固定目录句柄的 `NtSetInformationFile` 无覆盖重命名，不回退覆盖复制 |
| 持久化结果 | 保留目录同步 | 如实返回 `directory_synced=false`，不承诺断电目录持久化 |

Windows 私有文件在创建时限制为当前用户／SYSTEM；输出目录、暂存根和候选使用相应句柄锁，候选写入句柄未释放不得提交。清理只针对本次已确认身份的材料；停止未知、归属不确定或文件系统不支持所需语义时失败并保留材料。Job 归零不能替代文件可提交证明。

程序与工作目录固定为绝对路径，Windows 只执行原生 `.exe`，不通过 shell 接入 `.cmd`／`.bat`。逐项参数、环境块和标准句柄白名单归原生启动模块，不序列化或记录凭据。路径统一用 `dunce::simplified()`；Java 工作目录限制见[工具适配](java-tool-adapter.md)。

以上约束不构成管理员／同权限对手隔离，也不消除文件检查与按路径执行之间的全部竞态。

## 协议与取消

`private_json` 复用有界 JSON、身份和私有权限校验。`event_output` 负责有限队列及写入／刷新期限；输出失败、队列拒绝、线程异常或有限收尾未确认不能报告交付成功。

MCP 标准流统一在 `automation/transport`：输入 EOF、取消、输出错误或写入期限到期均触发原任务收尾。Windows 控制台回调只登记 Ctrl+C／Break，由业务线程清场，不新建控制台；静默且没有写入时不承诺立即检测输出读端关闭。强制结束不伪称优雅取消。

材料所有权、停止证明和无覆盖提交归原执行器，不用心跳停止、直属进程退出或析构成功替代整组确认。

## 桌面共包

| 平台 | 命令 | 共用资源 |
|------|------|----------|
| macOS | `.app/Contents/MacOS/shield` | `Contents/Resources/{tools,resources}` |
| Linux deb | `/usr/bin/mocika-shield-cli` | `/usr/lib/MocikaShield/{tools,resources}` |
| Linux AppImage | 完整 AppDir 的 `usr/bin/mocika-shield-cli` | 同一 AppDir 的 `usr/lib/MocikaShield/{tools,resources}` |
| Windows | 安装目录的 `mocika-shield-cli.exe` | 同目录 `{tools,resources}` |

Linux／Windows 用 `scripts/prepare_desktop_cli.py` 准备原生 x64 CLI 和生成的 Tauri 外部二进制配置；默认配置不引用未生成文件，普通 Cargo 检查不要求先共包。`SKIP_CLI_RELEASE=1` 只跳过独立维护压缩包，不跳过 GUI 内 CLI。macOS 保留完整 `.app` 构建和签名流程。

核心 `bundle_resources` 固定桌面命令的资源根，缺失即失败，不借源码、用户目录、PATH 或网络补资源。Linux 目录大小写按 Tauri 的 `productName`，与 Cargo 包名不同；契约测试及实际产物检查共同防止定位与模拟夹具一起写错。普通未改名的开发／维护 CLI 保留原查找方式。

AppImage 须保留完整解包目录，不能只复制命令；临时挂载结束后，挂载路径不再可用。构建不安装到维护者本机、不改 PATH、不启动 GUI 或 MCP。

## 验证与回退

快速检查覆盖格式、编译、协议和脚本契约；Java、原生生命周期及完整安装包分别手动验收，不增加每次提交的重任务。范围与证据统一见[验收表](../process/agent-desktop-platforms-plan.md)。

撤回按平台关闭新增特性与共包入口，不迁移或删除用户配置／证书库。版本、合并和发布另行确认，不把当前源码版本等同公开安装包已包含新增能力。

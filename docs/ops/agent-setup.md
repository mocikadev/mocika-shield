# 本地 Agent 接入与 Skill 安装

当前是开发分支的实验接入，尚未发布；公开 1.4.3 安装包不包含 `agent plan/protect`。首轮执行仅限 macOS；其他平台和额外客户端分别验收，不根据安装包或版本号推定已经支持。完整契约见[命令设计](../design/agent-cli-contract.md)，实际进展见[验收记录](../process/agent-integration-plan.md)。

## 目录与职责

```text
skills/mocika-shield/                 # 根目录直接可见，独立安装和升级
├── SKILL.md                          # 流程、自身版本和 CLI 协议兼容声明
├── references/cli.md                 # 随技能携带的命令/结果说明
├── scripts/resolve-cli.sh            # 只查找路径，不执行候选或保存配置
└── agents/openai.yaml                # 客户端界面元数据，不声明 MCP
docs/ops/agent-setup.md               # 面向用户的安装、配置与使用说明
docs/design/agent-*.md                # 维护者设计，不打入技能包
docs/process/agent-integration-plan.md # 阶段与实际验收记录
tests/agent-skill/cases.json           # 客户端行为验收场景，不打入技能包
scripts/tests/test_agent_skill.py      # 可搬迁性、引用与分发完整性检查
```

`skills/` 是技能的唯一源码入口，不是本仓库自动激活的隐藏开发技能目录。业务执行仍在 CLI 与共享核心，Skill 不包含二进制、JAR、资源包、证书、密码、测试 APK 或第二套执行器；小型路径定位脚本只供不同 Agent 复用同一查找规则，不包装加固任务。README 提供直接入口，不为未来 MCP 或其他平台预建空目录。

App 和 Skill 分开交付：macOS 应用只携带 GUI、CLI 与运行资源，技能由用户从仓库独立取得。应用内布局如下，不包含 Skill：

```text
MocikaShield.app/Contents/
├── MacOS/
│   ├── mocika-shield
│   └── shield
└── Resources/
    ├── tools/
    └── resources/
```

CLI 可在 GUI 关闭时使用。配置、输入和输出放在应用包及技能目录之外，不把安装目录作为工作目录；这是一项使用约定，不等于系统写保护。Skill 不在 App 构建资源清单中，没有技能源码也能构建 App，不需要为了更新操作指引而升级整个应用。

## 准备应用和本地签名配置

维护者按[构建说明](build.md)执行 `make build-gui` 生成完整 `.app`。将整个应用放在选定位置，不只提取内部 CLI；不要修改已交付的应用包或使用自动下载执行脚本来修复环境。

在本机准备独立 `shield-cli.toml`，格式与相对路径规则见[使用指南](../usage.md#cli-配置文件)；它不保存密码，不复用 GUI 的 `config.toml` 或 `shield.db`。受控流程要求显式输入、输出及原签名证书，输出父目录须已存在，首版只支持 V1–V3。

客户端宿主进程须在本地得到 `MOCIKA_SHIELD_KS_PASS`，可选 `MOCIKA_SHIELD_KEY_PASS`；如何安全注入取决于客户端。终端里设置环境变量不代表桌面启动的客户端也会继承。不要将密码粘贴到模型对话、技能文件、项目仓库或工具参数；缺凭据时请先解决宿主注入，不让 Agent 回显环境变量来排障。

## 命令定位与本机配置

Skill 按以下顺序定位 CLI：本次明确指定的绝对路径、宿主 `MOCIKA_SHIELD_CLI`、PATH 中的 `shield`，均没有时询问用户。明确配置失效或为空时不会借其他位置回退；PATH 有多个不同文件候选时要求选择。同文件链接或重复 PATH 项可去重，不递归扫描用户目录。

只需在使用者自己的 Agent 环境中配置一次路径。例如（替换为实际位置，不假定 App 在应用程序目录）：

```bash
export MOCIKA_SHIELD_CLI='/你的实际目录/MocikaShield.app/Contents/MacOS/shield'
```

这不是持久化安装命令；终端变量仅对继承它的进程有效。使用桌面 Agent 时按其宿主环境配置方式设置，不编辑技能文件，也不新增本工具配置字段。App 原地升级通常保留路径，移动后需要调整；技能升级不触碰这一外部配置。

技能内 `/bin/bash <已安装技能目录>/scripts/resolve-cli.sh [本次CLI绝对路径]` 只查找、不执行候选，成功输出一条路径，缺失/无效/歧义分别退出 `1/2/3`。它不证明候选来源可信或接口兼容。Agent 仍须核对安装来源、`--version`、`agent --help` 以及实际 JSON 的公共协议 `schema_version`，之后固定本次规划/执行路径；不要遍历或回显其他环境变量。

## 从仓库安装 Skill

安装来源为 `mocikadev/mocika-shield` 仓库的 `skills/mocika-shield/` 子目录。用户通过所用 Agent 或已有技能管理工具安装完整目录，按客户端支持选择项目级或用户级位置；不从 `.app` 中提取，不要求统一安装路径，也不要求额外使用本项目自制安装器。请保留参考、脚本和客户端元数据。

本次内容尚未推送或发布时，只能使用当前开发分支的本地文件，不能假定远端已可安装。后续从仓库安装时记录所用提交或引用，便于更新和回退；本工程不自动写入用户的项目或全局目录。

以 Codex 指定项目安装为例：

Codex 官方文档说明，项目级技能使用 `.agents/skills/`，可通过 `$技能名` 显式调用；技能变化未出现时可重新启动客户端。以下示例只安装到你选定的 Android 工程，不修改用户级技能或全局配置。[官方说明](https://developers.openai.com/codex/build-skills)

将已取得的 `skills/mocika-shield/` 整个目录复制到目标 Android 工程的 `.agents/skills/mocika-shield/`，保留内部布局。目标已有目录或链接时先核对，不覆盖；其他 Agent 使用其支持的技能目录，不照搬 Codex 的路径。这里只给出目录安装说明，不声称已验证所有客户端或管理工具。

## Skill 独立升级

技能版本保存在 `SKILL.md` 的 `metadata.version`，初始实验版为 `0.1.0`；`metadata.cli-protocol` 声明公共 CLI 协议 `1`，`metadata.platforms` 当前为 `macos`。这些是兼容声明，不是程序执行权限；技能版本不等于 App 版本，不能拿 App 的 1.4.3 推定接口已存在。

- 用技能管理工具安装的，采用该工具支持的更新/固定版本方式，保留仓库、子目录及实际提交/引用。不在此编造各管理工具的更新命令。
- 手动安装的，先核对变更并备份当前技能，再完整替换该技能目录，检查新增/删除文件；不覆盖外部 Agent 环境、`shield-cli.toml`、证书或密码。
- 更新后重新加载技能、核对 CLI 能力及公共协议；同协议、所需能力仍满足时，不要求同时升级 App。不兼容则停止并提示用户选择匹配版本，不擅自升级工具。
- 不在加固任务中更新技能/CLI，不新增自动更新服务，不运行自更新脚本。升级前的执行计划不跨更新复用，须重新规划。

## 开始使用

在目标工程的新会话中，先确认技能可被发现，再明确提供本机应用路径和文件位置。例如：

> 使用 $mocika-shield。工具在我指定的完整应用包内；请检查本机加固和证书环境，不执行加固，不读取或回显签名密码。

准备好自有 APK 和签名配置后：

> 使用 $mocika-shield，针对我指定的 APK、shield-cli.toml 和新输出路径做加固签名计划，先不要执行；说明阻断、警告和需要确认的地方。

确认计划及策略后，再授权执行。不把“安装技能”“查看计划”理解为授权安装 APK、发布、覆盖、排除 ABI 或上传资料。

## 客户端验收

使用 [场景清单](../../tests/agent-skill/cases.json) 逐项核对实际工具调用、进程终态和文件变化，不只检查模型答复。至少记录客户端版本、应用包来源/路径、Java 主版本、授权范围、是否出现覆盖或额外工具调用；记录中不含凭据、生产证书或业务 APK。不同客户端分别记录，不将一个客户端的 CLI 运行结果当作其他客户端通过。

可以直接由当前 Agent 显式读取技能并在临时项目验证，不要求先另开会话；本项目已完成一次[当前会话显式使用验收](../process/agent-session-validation.md)，包含实际工具调用、解包阶段取消及模拟升级配置保留。该结果不同于仅跑脚本，也不证明新会话自动发现/选择、其他 Agent 或各客户端按钮/断开行为。其他客户端的目录与交互按需补测，不把它们统一设为已验证流程继续推进的前置条件；未经确认仍不安装到全局。

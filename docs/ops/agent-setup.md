# 本地 Agent 接入与 Skill 安装

> 当前调整：CLI 已有[共享管理、签名、普通及受控加固](shared-data-cli.md)，独立 Skill 0.3.0 提供单次受控独立签名指引；MCP 已接入[共享来源与证书白名单](../design/shared-mcp.md)。单次签名及缺用户名兼容已通过本机完整包和当前 Agent 实调；当前功能分支另接入设置／证书偏好及默认切换，实际进度见[管理任务](../process/shared-mcp-management-plan.md)。下文原外置配置流程继续兼容；公开安装包、宿主原生注册及未完成验收不得据此推定已更新。

CLI 与 Skill 已随 PR #152 合并主干，仍是尚未发布的实验接入；公开 1.4.3 安装包不包含 `agent plan/protect`。首轮执行仅限 macOS；其他平台和额外客户端分别验收，不根据安装包或版本号推定已经支持。完整契约见[命令设计](../design/agent-cli-contract.md)，实际进展见[验收记录](../process/agent-integration-plan.md)。本地 MCP 按[专项计划](../process/local-mcp-plan.md)开发，尚不作为可安装能力。

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

共享来源通过已保存的 GUI／CLI 证书和偏好工作，无须另建项目或注入密码，见[共享资料说明](shared-data-cli.md)。外部临时材料仍使用独立 `shield-cli.toml`，格式见[使用指南](../usage.md#cli-配置文件)；不保存密码，不把 GUI 文件当项目配置。受控流程仍要求明确输入、输出及原签名证书，父目录须已存在，仅支持 V1–V3。

外部材料、导入或创建新证书时，宿主须在本地安全取得 `MOCIKA_SHIELD_KS_PASS` 和可选 `MOCIKA_SHIELD_KEY_PASS`；终端变量不代表桌面客户端会继承。不将密码粘贴到模型对话、技能文件、项目仓库或工具参数；缺凭据先解决本地注入，不回显环境排障。已保存共享材料不需要重复注入。

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

技能源码已在远端主干，可记录所用提交或引用后独立安装；但须自行准备匹配的开发应用包，不能假定公开 1.4.3 包已支持新命令。记录实际来源便于更新和回退；本工程不自动写入用户的项目或全局目录。

以 Codex 指定项目安装为例：

Codex 官方文档说明，项目级技能使用 `.agents/skills/`，可通过 `$技能名` 显式调用；技能变化未出现时可重新启动客户端。以下示例只安装到你选定的 Android 工程，不修改用户级技能或全局配置。[官方说明](https://developers.openai.com/codex/build-skills)

将已取得的 `skills/mocika-shield/` 整个目录复制到目标 Android 工程的 `.agents/skills/mocika-shield/`，保留内部布局。目标已有目录或链接时先核对，不覆盖；其他 Agent 使用其支持的技能目录，不照搬 Codex 的路径。这里只给出目录安装说明，不声称已验证所有客户端或管理工具。

## Skill 独立升级

技能版本保存在 `SKILL.md` 的 `metadata.version`，当前开发版 `0.3.0` 在共享受控、资料管理及普通签名基线上，增加单次受控独立签名；公共 CLI 协议仍为 `1`，识别计划版本 `2/3/4`，当前平台 `macos`。声明不等于执行权限或 App 版本，不能用 App 1.4.3 推定接口存在。按任务检查对应能力，不把缺少独立签名或管理能力的旧 CLI 当成所有既有加固流程都不可用。

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

## 本地 MCP 开发验证

2026-11-09：开发中的 MCP 默认提供 `shield_doctor`、`shield_preflight`、`shield_plan` 三项只读工具；宿主显式允许后额外提供受控执行和状态查询。它与 Skill 独立，不要求安装技能，也不由技能启动服务。源码的 macOS `.app` 构建已显式启用 MCP，公开安装包尚未更新，不能用 App 版本号推定入口存在；用对应包内 `shield mcp serve --help` 检查入口。

完整应用包用 `bash scripts/build-macos-app.sh` 构建；宿主的程序字段指向用户实际保存位置的 `MocikaShield.app/Contents/MacOS/shield`，参数按独立数组传入，不通过 shell、不假设安装在 `/Applications`。升级后如应用位置变化，由用户更新宿主路径；不得只提取裸 CLI，或让 Agent 在运行中覆盖应用。GUI 无须启动。参数示意：

```text
/实际保存位置/MocikaShield.app/Contents/MacOS/shield --config /可信配置目录/shield-cli.toml mcp serve --input-root /授权输入目录 --output-root /授权输出目录 --timeout-seconds 120
```

- 输入、输出根目录须已存在，启动参数使用绝对路径；不从输入目录推断配置或工具。工具调用只传各自根目录内的相对路径。
- 签名配置可不提供，此时环境检查/预检仍可用，计划返回缺配置错误。签名配置必须在输出根目录之外；固定配置变化后需重启服务，不能在工具调用中切换。
- 密码仍由宿主在本机注入既有环境变量，不能写在 MCP 工具参数中。无凭据需求的检查子进程不继承签名密码。只读规划也会调用 Java/keytool，产生本次临时材料，但不交付最终 APK。
- 默认只读；由宿主在启动参数额外加入 `--allow-execution` 后，才注册 `shield_protect` 和 `shield_execution_status`，此时固定签名配置为必需项。工具调用不能切换这项权限；用户仍须授权单次执行，不能把启动服务当作批准任意任务。
- 本轮使用 `2025-11-25` 协议进行真实标准输入输出连接测试，尚未把服务注册进当前 Agent 的工具列表；连接测试不等于实际宿主的发现、授权和按钮行为已验收。
- 不自动修改任何 Agent 全局配置，不监听网络端口，不改变系统 Java。各宿主配置入口和授权按钮分别验收，不把真实标准流客户端验证写成宿主已接入。

只做源码协议调试时也可用 `cargo build -p shield-cli --features mcp-server --locked`；这不替代完整应用包验收。源码默认构建不开放 MCP，服务存活期间不要原地重新编译/替换执行器或修改固定配置。

开发验证流程为：`shield_plan` 返回 `plan_id`、`plan.sha256` 和有效期；确认后调用 `shield_protect`，只传 `plan_id` 与 `expect_plan_sha256`，不再次传路径、配置、密码或覆盖选项。`shield_execution_status` 只传 `plan_id`，执行期间和请求取消后都可在同一连接查询。计划最多保留 16 条，未消费计划 10 分钟到期，结束记录另保留 10 分钟；不建立持久任务库。

开始尝试即消费计划，输出占用、配置变化、工具失败或结果未知后均不得对同一计划重试。MCP 取消使用原请求 ID 的 `notifications/cancelled`；不等待被取消请求的响应，改查本会话状态。已提交产物不会因晚取消被删除；连接中断后不可通过新连接恢复状态，也不能自动换输出名重跑。状态查询不代表重新验证环境。需要阶段通知时由客户端为调用提供 `_meta.progressToken`，阶段序号不是耗时百分比。

若出现提交/结果未知或清理未确认，本会话暂停新的规划与执行，返回 `SESSION_RECOVERY_REQUIRED`，状态中 `session_execution_blocked` 为真。不能改用另一个计划或等待记录过期来恢复；先人工检查本机进程、临时材料和产物，再由用户重新建立会话，不得自动重连或重启。已有成功产物不会删除，状态查询和只读检查继续可用。明确未启动 CLI 的取消/输出占用等不需要清理，不会仅因此锁定会话，但原执行计划仍已消费。

### 开发分支的单次独立签名

以下能力已通过当前 Mac 的源码标准流及完整 `.app` 验证，宿主注册仍另验；独立 Skill 0.3.0 已同步指引，不随 App 打包。预先在 GUI／CLI 保存证书后，宿主一次配置共享范围和原执行开关，不增加签名专用开关：

```text
shield mcp serve --use-shared-settings --allow-execution --input-root /授权输入目录 --output-root /授权输出目录 --allow-certificate 证书编号
```

`shield_sign` 一次调用只需：

```json
{"input":"应用.apk","output":"应用-signed.apk"}
```

可以另传 `certificate_id` 选择已有允许项。省略时使用范围内默认项或唯一允许项；多项无法确定则返回固定错误，不猜选或挂起询问。只做独立签名，不加固、不覆盖，首版仅支持 V1–V3；不需要先调用 `shield_plan`、复制摘要或再次确认。宿主自身的工具授权策略仍由宿主管理。

返回的 `plan_id` 只是本会话任务查询引用，不能提交给 `shield_protect`。若客户端需要运行中或取消后查询，在同一次签名请求提供 `_meta.progressToken`，保存阶段通知 `_meta["mocika-shield/plan_id"]` 后，用原状态工具查询；不为取得引用增加前置调用。未请求进度也能正常完成签名，并在结果中取得引用。

独立签名基线为共享执行 7 项、共享只读 4 项工具；当前管理扩展另见下节。默认项查询与签名共用一次总期限。缺信息／版本不支持等普通错误，在 CLI 有实际清场证据时不会锁死会话；错误修正后的下一次明确请求可以继续处理，不允许自动重试、绕过未知锁定或自动重启服务。

### 开发分支的共享资料管理

当前功能分支新增 `shield_settings` 查询业务设置；共享模式启用原 `--allow-execution` 后，还提供 `shield_update_settings`、`shield_update_certificate`、`shield_set_default_certificate`。无需新的启动开关、前置计划或逐次确认；服务只在明确保存／管理意图下执行，宿主原有审批策略不变。共享只读为 5 项、共享执行为 11 项，原外置模式仍为 3／5 项。公开安装包尚未包含，能力以命令及工具发现为准。

设置只包括模式、策略、自动签名意图及加固页专用证书，省略保持；两个可空字段的 `null` 分别恢复继承与清除专用选择，不删除证书。示例参数：

```json
{"runtime_mode":"android_api19","environment_policy":"compatible","sign_after_protect":null,"certificate_id":null}
```

证书修改只接 `certificate_id` 及可选 `name`、`note`、`v1/v2/v3/v4`、`auto_sign`；全局默认切换只接允许范围内的明确编号，不修改加固页专用选择。不接受路径、Alias、密码、导入／创建／校验／删除，不回显名称、备注或材料。设置中的范围外证书编号会隐藏并标记 `outside_scope`，不是自动改为默认项。

成功写入回执为 `change_state:confirmed`；`not_attempted` 表示明确未尝试。`unconfirmed` 表示可能已保存，不能认为回滚、自动重试或开新连接绕过；可查询实际偏好，当前会话继续停写。管理命令不使用项目 `--config`，不会把临时覆盖保存成默认；已固定的加固计划仍使用原身份。设计及实际验收见[首批管理方案](../design/shared-mcp-management.md)与[任务记录](../process/shared-mcp-management-plan.md)。

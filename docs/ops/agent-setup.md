# 本地 Agent 接入与 Skill 安装

> 当前主干已合入 CLI、独立 Skill、MCP 和共享资料管理，先以已验证的 macOS 开发应用包使用。**公开 1.4.3 安装包尚不包含这些新增入口。** 源码构建仍显示 1.4.3，不能只看版本号判断能力，须核对对应命令帮助和工具发现。交付状态与证据见[收口审查](../process/agent-delivery-review.md)。

## 选择入口与共享资料

| 入口 | 使用方式 |
|---|---|
| CLI | 本机脚本直接调用应用内 `shield`，不要求 GUI 打开 |
| Skill | 用户独立安装 `skills/mocika-shield/`，Agent 按流程调用 CLI，不包含另一套执行实现 |
| MCP | 宿主按需启动同一 CLI 的 `mcp serve`，通过标准输入输出发现和调用工具，不监听网络端口 |

三者可独立使用，不强制同时安装 Skill 和 MCP。GUI、CLI 和共享 MCP 复用本地 `config.toml`、`shield.db` 与证书；查询和保存通过[共享服务](../design/shared-application.md)，不让模型直接读取数据库或解密密码。GUI 无需常驻；切页、恢复焦点或下次查询时可见新偏好，活动任务仍使用固定快照。

本批验收限 macOS，普通源码 CLI 默认不启用 `agent-executor`／`mcp-server`；macOS 共包脚本会显式启用。Linux／Windows、Intel／通用架构、额外 JDK 和宿主原生注册分别验收，不将当前 Mac 的结果扩写为全部通过。

## 准备完整应用

按[构建指南](build.md)先准备壳资源，再运行 `make build-gui` 或 `bash scripts/build-macos-app.sh`。使用完整 `.app`，不要只提取裸二进制、事后替换包内文件或让 Agent 自行下载替代工具。应用不一定在 `/Applications`，以下路径都须替换为真实位置：

```bash
SHIELD_CLI='/你的实际目录/MocikaShield.app/Contents/MacOS/shield'
"$SHIELD_CLI" --version
"$SHIELD_CLI" agent --help
"$SHIELD_CLI" mcp serve --help
```

应用布局为：

```text
MocikaShield.app/Contents/
├── MacOS/
│   ├── mocika-shield              # GUI
│   └── shield                     # 独立 CLI，含本地 MCP 入口
└── Resources/
    ├── tools/                    # 共用 JAR
    └── resources/                # 标准／工控资源
```

Skill 不在应用包中。输入、输出、可选项目配置和技能安装目录留在包外；输出父目录须已存在，不能覆盖原包或已有目标。完整构建和签名只说明应用包就绪，不等于 Android 设备业务已验。

## 最简共享 MCP 配置

### 使用 GUI 已保存的证书

先在本机查看证书，取所选记录的 `id`；它不是 Alias，不需要再次输入密码：

```bash
"$SHIELD_CLI" cert list
```

列表不含密码，但有名称、路径和 Alias，只在本机查看，不把整份摘要贴到对话中。未指定 `--app-home` 时即使用 GUI 的原资料；该参数只供宿主显式隔离测试，不是共享 GUI 资料的必填项，也不能通过复制数据库代替迁移。

将下面的路径和证书编号替换为本机实际值，再把服务定义放入宿主支持的 **stdio MCP** 配置位置。`command` 是程序绝对路径，`args` 是独立参数数组，不经过 shell；具体外层包装字段由客户端决定，不声称所有客户端格式相同：

```json
{
  "command": "/你的实际目录/MocikaShield.app/Contents/MacOS/shield",
  "args": [
    "mcp", "serve",
    "--use-shared-settings",
    "--allow-execution",
    "--input-root", "/你的APK目录",
    "--output-root", "/你的产物目录",
    "--allow-certificate", "所选证书编号"
  ]
}
```

输入、输出根目录必须预先存在，使用明确的包外绝对路径；不要选择整个用户目录等过宽范围。只读接入删去 `--allow-execution`。服务默认总期限为 120 秒，处理较大 APK 时可由宿主预先设置 `--timeout-seconds`（1～3600 秒），不会因进度自动续期。

共享模式不需要 `shield-cli.toml` 或密码环境；已有证书签名由本地服务读取已保存密码。允许多个旧证书时重复 `--allow-certificate`，总范围最多 64 项。服务能力由宿主一次配置，工具不能自行提权；明确的本次任务不要求第二次确认，宿主本身的审批策略保持不变。

### 初次没有证书

可省略整组 `--allow-certificate`，表示不授权任何旧记录，不是开放整库。用户明确要求导入／创建时，仅这两种固定调用使用宿主本机安全预置的 `MOCIKA_SHIELD_KS_PASS` 和可选 `MOCIKA_SHIELD_KEY_PASS`；不要将密码填入工具参数、技能、仓库或对话。

只有导入／创建完整确认成功的新编号才加入当前会话，可直接用于签名、加固和管理，无须为使用新编号而重启。删除成功后撤销编号；新服务会话须由宿主显式配置要沿用的旧编号。缺安全凭据时明确停止，不猜密码、不自动生成密码或重启；更换预置密码由宿主更新环境。详见[生命周期方案](../design/mcp-certificate-lifecycle.md)。

## 当前工具与能力

| 模式 | 不加执行开关 | 加原 `--allow-execution` |
|---|---:|---:|
| 共享来源 | 5 项 | 15 项 |
| 原外置配置来源 | 3 项 | 5 项 |

以当前实际 `tools/list` 为准，不把旧阶段工具数量当作新增权限。共享模式的完整工具清单：

| 分组 | 工具 | 说明 |
|---|---|---|
| 检查与规划 | `shield_doctor`、`shield_preflight`、`shield_plan` | 默认可用；规划会短暂准备／回收工作材料，不交付 APK |
| 共享查询 | `shield_certificates`、`shield_settings` | 默认可用；只返回有限业务字段，不返回名称、路径、Alias 或密码 |
| 受控任务 | `shield_protect`、`shield_sign`、`shield_execution_status` | 开启执行后可用；状态查询本身只读，取消／未知状态不自动重跑 |
| 偏好与默认 | `shield_update_settings`、`shield_update_certificate`、`shield_set_default_certificate` | 明确保存意图后一次调用完成，不另做计划 |
| 证书生命周期 | `shield_verify_certificate`、`shield_import_certificate`、`shield_create_certificate`、`shield_remove_certificate` | 校验也会保存状态；删除只移除记录、始终保留文件 |

能力共享不代表每个入口已经有全部操作：

| 操作 | CLI／Skill | 当前 MCP |
|---|---|---|
| 仅加固、不签名 | 可按明确要求走普通 `protect`，不冒充受控保证 | 尚未开放，不擅自加上签名或切换成普通命令 |
| 清空全局默认证书 | `cert clear-default` | 尚未开放；只能把明确允许项设为默认 |
| 持久化 GUI 输出目录偏好 | `config set` | 尚未开放；任务输出使用宿主根目录及本次相对路径 |
| 受控独立签名 | `agent sign` 单次执行 | `shield_sign` 单次执行；均仅 V1–V3，不自动关闭 V4 |

这些差异不在本次文档收口中补功能，也不作为已完成能力继续合并的前置条件。普通命令详见[共享资料 CLI](shared-data-cli.md)。

## 常用任务示例

### 只检查环境

> 检查签名和证书工具是否就绪，不处理 APK、不保存设置。

可调用 `shield_doctor`，参数为 `{"operation":"sign"}` 和 `{"operation":"certificate"}`；`ready` 只说明对应依赖检查，不等于真实私钥签名已通过。

### 只签名，不加固

> 用已允许的共享证书，将 app.apk 签名到新的 app-signed.apk，不安装或发布。

直接单次调用 `shield_sign`：

```json
{"input":"app.apk","output":"app-signed.apk"}
```

省略编号时使用唯一允许项，或多项范围内的全局默认项；无法确定就返回有限错误，不猜证书。也可显式加 `certificate_id`。不需要先调用 `shield_plan`、复制摘要、输入保存过的密码或再确认一次。

### 加固并签名

> 使用已允许的证书和保存的目标系统／保护策略，将 app.apk 加固并签名到新的 app-protected.apk。

Agent 先调用 `shield_plan`，提供相对 `input`、`output` 和明确的 `certificate_id`；用户已授权执行且风险已明确时，自动把原返回的 `plan_id`、`plan.sha256` 传给 `shield_protect` 的 `plan_id`、`expect_plan_sha256`。不要求用户手抄摘要或重复指定路径／材料。用户只要求评估或规划时停在计划，不执行。

可选 `runtime_profile` 使用 `standard`／`android-api19`；省略则继承保存的模式。ABI 排除仍按本次明确意图提供，不自动排除。计划一次尝试即消费，不能重用失败或未知任务的引用。

### 保存偏好或管理证书

> 将今后的目标系统改为兼容 Android 4.4，保护策略保持标准保护，不执行加固。

调用 `shield_update_settings`，参数为 `{"runtime_mode":"android_api19","environment_policy":"compatible"}`。注意保存字段 `runtime_mode` 使用下划线，与规划参数的连字符不同；省略字段保持。`sign_after_protect:null` 恢复继承，`certificate_id:null` 清除加固页专用选择，不删除证书或清空全局默认。

证书偏好工具只收编号及名称／备注、V1–V4、自动签名偏好，不可更换材料。导入使用输入根内相对 `keystore`、名称、类型和可选 Alias；只一个 Alias 时可自动选择，文件上限 16 MiB，拒绝链接、跳转和会被路径规范处理改变的末尾空白。创建只收托管文件名、不接保存目录，同名不覆盖。示例参数：

```json
{"name":"发布证书","keystore":"证书/release.p12","keystore_type":"pkcs12"}
```

创建可用 `{"name":"发布证书","file_name":"release.p12","alias":"release","dname":"CN=本地签名","keystore_type":"pkcs12"}`。两者可选 `note`、`set_default`、`v1/v2/v3/v4`、`auto_sign`，默认不改全局默认项，V1–V3 开、V4 关、自动签名偏好开。创建／导入仍须满足本机凭据条件。

明确要求删除某一记录时，`shield_remove_certificate` 只需 `certificate_id`，无第二次确认字段；文件保留，也不会自动选择替代证书。管理说明和结果范围见[首批方案](../design/shared-mcp-management.md)及[生命周期方案](../design/mcp-certificate-lifecycle.md)。

## Skill 独立安装与升级

唯一源码为根目录 [skills/mocika-shield/](../../skills/mocika-shield/SKILL.md)。将整个目录安装到所用 Agent 支持的项目级或用户级技能位置，保留 `SKILL.md`、`references/`、`scripts/`、`agents/`；不从 App 提取，不放入本仓库的隐藏开发技能目录，不要求各客户端使用相同路径。已有目标先核对和备份，不直接覆盖。也可使用用户已有的技能管理工具，本项目不自研安装器。

当前 Skill 为 `0.3.0`、公共 CLI 协议 `1`，识别加固计划 `2/3` 和独立签名计划 `4`；声明不等于 App 已具备所有接口。它按“本次明确绝对路径 → 宿主 `MOCIKA_SHIELD_CLI` → PATH 中 `shield` → 询问”定位，无效明确配置不回退，多文件歧义停止，不遍历用户目录。例如由用户给宿主设置：

```bash
export MOCIKA_SHIELD_CLI='/你的实际目录/MocikaShield.app/Contents/MacOS/shield'
```

终端环境不代表桌面客户端会继承，也不是持久安装。定位脚本 `scripts/resolve-cli.sh` 只查找、不执行候选；成功后仍需核对可信来源、完整资源及所需帮助入口。共享资料／仅签名按包内参考处理，不由 Skill 注册或启动 MCP。

更新时按用户选择的仓库提交／版本或管理工具更新完整技能，保留外部路径、证书和项目配置。更新后重新加载并核对能力；不在业务任务中自更新、不跨升级复用执行计划，不要求 Skill 与 App 同版本升级。用户可明确说：

> 使用 $mocika-shield，工具位于我指定的完整应用包中；用现有共享证书仅签名到指定新文件，不执行加固。

## 原外置配置模式

旧入口继续兼容：不指定 `--use-shared-settings`，由宿主提供包外的 `--config /可信目录/shield-cli.toml`；默认三项检查／规划工具，允许执行后五项。外置执行要求固定项目配置及安全预置密码，不读取共享库来拼接材料。它不提供共享独立签名或资料管理工具，不把共享模式的配置条件套用到这里。

项目文件不保存密码，不把 GUI 的 `config.toml` 当项目文件；固定文件发生变化会阻断原计划。外置流程属于兼容选择，不是使用 GUI 已保存证书的前置条件。

## 结果、取消与验收边界

- APK 成功必须有完整的 `succeeded`、`output_state:published` 及产物证据；清理或目录同步异常照实报告。签名／ZIP 对齐通过不等于设备安装、启动或全部业务通过。
- 管理 `change_state:confirmed` 表示结果已确认；`not_attempted` 表示明确未尝试；`unconfirmed` 表示可能已保存，不能称回滚或自动重试。校验失败但状态明确保存仍是失败，不误报成功，也不因此锁死会话。
- 未确认停止、提交或修改时会话停写并返回 `SESSION_RECOVERY_REQUIRED`，查询仍可用。先人工核对本机状态，不自动重连、改名输出、重建计划或重启绕过。
- MCP 取消使用本次请求的 `notifications/cancelled`。客户端可能不再收到被取消调用的结果；受控 APK 任务可用同连接的 `shield_execution_status` 查询。独立签名需要取消后查询引用时，在原请求提供进度标识并保存通知中的任务编号，无须前置计划；管理取消不伪造为 APK 状态记录。
- 服务总期限、连接关闭和有限收尾沿用既有契约，晚取消不删除已发布产物。源码调试时勿在服务存活期间原地编译／替换 CLI 或固定配置。

当前 Agent 已完成显式 Skill 使用、临时标准流工具调用及真实签名，见[会话记录](../process/agent-session-validation.md)和[交付审查](../process/agent-delivery-review.md)；完整包、旧证书复用与协议专项分别有实际证据。这不同于宿主原生注册、新会话自动发现、每个客户端审批／取消按钮或跨平台验收。未经确认不写任何客户端全局配置、安装技能或修改 Java；本批文档调整不要求重复已通过的完整测试。

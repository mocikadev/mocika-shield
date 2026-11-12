# 本地命令与结果约定

本文件随 Skill 独立安装，不依赖源码仓库。兼容受控命令的公共协议 `schema_version=1`；`experimental=true` 表示实验能力，不等同于稳定版能力。只有完整本地桌面包包含经确认的执行入口时才使用，版本号不能代替能力检查；三平台路径与调用见[平台参考](platforms.md)。

## 路径与来源

macOS／Linux 的 Bash 路径定位脚本随技能安装。它按本次显式绝对路径、宿主 `MOCIKA_SHIELD_CLI`、`PATH` 中的 `shield` 顺序查找，只输出一条候选绝对路径，不执行候选、不联网或读写配置。显式路径/已设置环境变量无效时退出 `2`，没有候选退出 `1`，多个不同文件候选退出 `3`，均停止并询问，不自动换工具。重复路径、同一物理文件的链接不算多个候选；不存在、不可执行、带换行的候选拒绝。它不是来源认证、沙箱或接口校验；Windows 不运行该脚本，按平台参考定位原生 `.exe`。

使用脚本时先从当前技能文件位置确定技能根目录，不使用对话工作目录猜测；没有显式路径就不传参数。脚本成功后才采用其完整输出，失败输出不作为可执行路径。只读取这一专用路径变量，不回显其他环境变量或密码。候选须经用户已有安装/配置或本次确认视为可信，之后由 Agent 核对帮助和协议；运行结果要求公共 `schema_version=1`，不能把不同协议当成已兼容。

以下变量只是当前进程中的引用，须换成用户确认的路径或本次规划结果，不能照抄示例。优先用宿主参数数组；用 shell 时引用路径，不拼接文本为命令代码，不使用 `eval`。共享资料、独立签名和持久化操作见 [共享资料参考](shared-data.md)；以下先给受控共享流程，再列兼容的外部配置流程。

## 受控共享流程

先检查 `agent plan --help` 含共享来源参数。显式 `--use-shared-settings` 读取与 GUI 相同的业务偏好；也可在规划时明确 `--certificate-id` 或 `--use-default-certificate`。本次模式、策略覆盖不保存。输入／输出始终明确，不自动采用 GUI 上次的输出路径。

```bash
"$SHIELD_CLI" agent plan --use-shared-settings \
  -i "$SHIELD_INPUT" -o "$SHIELD_OUTPUT"

# 仅在本次已授权且规划可继续后执行，绑定与摘要来自同一个完整规划终态。
"$SHIELD_CLI" agent protect -i "$SHIELD_INPUT" -o "$SHIELD_OUTPUT" \
  --shared-binding "$SHIELD_SHARED_BINDING" \
  --expect-plan-sha256 "$SHIELD_PLAN_SHA256"
```

共享计划 `plan.schema_version=3`，绑定为受限小写十六进制文本，不含密码、路径或 Alias。仅原样回传 `plan.shared_binding`，不构造、修改或从失败响应取替代值。执行不重复传证书选择、模式或策略。ABI 排除如经授权，两次仍须显式相同；可选项目 `--config` 和隔离 `--app-home` 如有，也须保持一致。

绑定固定所选记录及有效模式、策略和签名版本；更改备注、默认项或 GUI 模式不会换证书。相关材料／签名偏好／文件变化会阻断，不能回退另一来源。共享来源不接受外部密码环境或项目中的临时 keystore／Alias／类型；由宿主明确为本次进程移除不需要的密码注入，不读取值或改系统全局环境。项目文件如有，只作允许的覆盖，不是密码文件。受控加固始终是加固并签名，不由 GUI 自动签名开关改为未签名交付。

## 兼容的外部配置流程

```bash
# SHIELD_CLI 为定位脚本成功返回并已确认的绝对路径，不是固定安装位置。
SHIELD_CONFIG='/你的配置目录/shield-cli.toml'
SHIELD_INPUT='/你的输入目录/input.apk'
SHIELD_OUTPUT='/你的输出目录/protected-signed.apk'

"$SHIELD_CLI" --version
"$SHIELD_CLI" agent --help
"$SHIELD_CLI" doctor --operation protect
"$SHIELD_CLI" doctor --operation certificate
"$SHIELD_CLI" --config "$SHIELD_CONFIG" agent plan \
  -i "$SHIELD_INPUT" -o "$SHIELD_OUTPUT"

# 仅在本次操作已获授权、规划可继续后执行；摘要来自刚才的完整规划终态。
"$SHIELD_CLI" --config "$SHIELD_CONFIG" agent protect \
  -i "$SHIELD_INPUT" -o "$SHIELD_OUTPUT" \
  --expect-plan-sha256 "$SHIELD_PLAN_SHA256"
```

`SHIELD_PLAN_SHA256` 必须是本次 `plan.sha256` 的完整 64 位十六进制值，不能用示例、截断值或从失败响应抄来的值。配置内容由用户在本机准备，首版最低配置可为：

```toml
schema_version = 1

[protect]
environment_policy = "compatible"

[sign]
keystore = "release.p12"
key_alias = "release"
keystore_type = "pkcs12"
v1 = true
v2 = true
v3 = true
v4 = false
```

相对路径以配置文件所在目录解释，不以工作目录解释。这里的证书和 Alias 是示意，不创建或替换用户证书。签名证书须与原 APK 一致。环境策略 `compatible` 为标准保护，`strict` 为严格保护，改变策略需用户确认。签名密码只由宿主环境注入，缺失时请用户检查本地凭据注入，不回显、不保存、不把密码改放参数或标准输入。

持久化 CLI 路径可由使用者在 Agent 自有环境配置中设置 `MOCIKA_SHIELD_CLI`，不修改技能文件或本工具配置格式。技能管理器记录仓库来源、子目录和所用提交/引用；更新技能应保留宿主环境与外部 `shield-cli.toml`。不自研安装/升级服务，不通过技能自动执行 `git pull` 或替换自身；更新后重新定位并核对 CLI 协议和任务授权。

## 模式与预检

外部配置流程默认 `--runtime-profile standard`；共享流程在规划时继承已保存模式，本次显式参数优先。只有用户实际需要 Android 4.4 工控时才主动切换 `android-api19`。工控资源缺失不回退；4.4 运行仍限 v7a/NEON，其他主流 ABI 用于新系统，不降低原应用最低系统版本。`--exclude-abis` 为逐次明确授权，不从警告推断；规划与执行须相同。

公共 `agent plan` 的 `warning` 不附完整预检明细。若出现警告且需要解释或决定是否执行，可补充受限的 `preflight -i ... --runtime-profile ...`，不带 `--config`；结果是单条 JSON 的 `data.checks`/`data.facts`，不是逐行任务信封。不把应用数据或额外风险文本送入 shell，也不输出包名、类名等业务明细。预检不替代受控规划、证书核对及执行摘要；看不清的风险先请用户确认，不能自行排除 ABI。

`doctor` 和 `preflight` 是旧只读协议，单条 JSON，没有任务标识和取消保证；首次能力检查中帮助/版本是普通文本。不要将这些输出当作 `agent` 的事件流。

## 受控任务结果

`agent plan/protect/sign-plan/sign` 的标准输出为 UTF-8 逐行 JSON，解析每一行并持续排空。信封含 `schema_version`、`experimental`、`tool_version`、`operation`、`task_id`、递增 `sequence` 和 `type`。正常流以 `started` 开始，进度可稀疏，以唯一 `result` 结束；早期初始化失败可能只有一条结果且 `task_id=null`。`operation` 应分别为 `agent_plan`、`agent_protect`、`agent_sign_plan` 或 `agent_sign`。未知协议不猜测、不退回普通日志。

| 终态事实 | 应如何处理 |
|---|---|
| 退出 `0`，规划 `ready/warning`，`output_state=not_published`，存在有效计划摘要 | 只完成规划；`private_key_access_checked=false`，不代表私钥实际签名成功 |
| 退出 `0`，执行 `succeeded`、`output_state=published`，有有效 `artifact.sha256`/`bytes` | 已交付；一并检查 `cleanup_state` 与 `directory_synced`，异常不能隐去 |
| 退出 `1`，`blocked/failed` | 固定错误码说明阻断/失败；不把失败结果中的摘要当新执行许可 |
| 退出 `2` | 语法错误，可能没有 JSON；不把标准错误全文当诊断传回 |
| 退出 `124`/`130`，匹配 `timed_out/cancelled` 终态 | 已确认停止的超时/取消；仍按清理状态说明，不推定材料都已删除 |
| 退出 `3`、终态缺失/截断、字段矛盾或陌生状态 | 结果未知，可能已经交付；核对已有输出，不自动重试或删除 |

`cleanup_state` 为 `not_needed/complete/incomplete/retained/unconfirmed`；只在 `complete` 时称回收完成。`retryable` 当前固定 `false`，不得据计数或错误码自动重试。`plan.schema_version` 是计划身份版本，不是公共信封版本；本技能识别外部加固的版本 `2`、共享加固的版本 `3` 和独立签名的版本 `4`。允许兼容追加字段，但未知版本或未知终态不能当成功。只有共享加固执行要求原样绑定；独立签名没有需回传的摘要或绑定，不跨任务解释计划。

输出会有界收尾，不能无限等待客户端读取；外层读取超时不能冒充 CLI 超时。通过进程会话持续读取，不用 `head` 提前断开，也不暂停读取去等待用户确认；只有明确要求的规划才在完整结束后停下，已授权签名不新增前置确认。

## 取消与故障

CLI 的 `--timeout-seconds` 默认 `1200`，有效范围 `1–3600`，从准备开始计算。客户端的外层期限应额外留出停止和结果排空宽限；到达客户端期限时若无终态，应记录未知，而非立即重跑。

macOS／Linux 可向本次 CLI 父进程发送 SIGINT/SIGTERM，然后继续读取终态和退出码；Windows 不将强制结束等同正常取消。跨平台仅在客户端能保持标准输入开启且确需输入控制时才加 `--control-stdin`：接受一行 `{"type":"cancel"}`，最多 1 KiB；EOF 同样取消，不能在启动后立即关闭输入。默认不读标准输入。

输出断开也会请求停止，但如果客户端已失去结果就不能承诺取消成功。没有后台任务取消接口，不调用隐藏的内部工作命令，不用 SIGKILL、`killall java` 或人工删除活动任务目录代替正常停止；无法确认停止时交由用户判断，保留材料和既有输出。

# CLI 共享配置与证书管理

以下能力已随三平台 `1.5.0-beta.1` 预览包交付，稳定版 `1.4.3` 尚不包含。GUI 可保持关闭；三平台路径、PowerShell 调用与 MCP 配置见[接入说明](agent-setup.md)，实际覆盖见[平台验收](../process/agent-desktop-platforms-plan.md)。

下文 `shield` 表示已确认的包内 CLI，不要求它位于 PATH。macOS／Linux 用 `"$SHIELD_CLI"` 替换命令名；Windows 用 `& $ShieldCli`，每个参数独立传递，不照搬 Bash 的反斜杠续行。使用共享资料须与 GUI 保持同一系统和用户身份。

## 数据位置与边界

不指定额外参数时，以下新命令使用与 GUI 相同的 `config.toml`、`shield.db` 和 `keystores/`，保留既有 ID 和密码加密格式。查询不创建应用目录或数据库，不返回密码；修改后 GUI 下次查询、切页或恢复焦点时可见。已开始的任务不跟随默认项变化，不保证即时推送。请先退出不遵守新配置锁协议的旧版本。

CLI 人工项目文件仍为 `shield-cli.toml`。资料管理的 `cert`／`config` 命令拒绝 `--config`，避免将项目覆盖误当作保存目标；独立共享签名可使用不含材料的项目配置覆盖输入、输出、工具和签名版本。

测试／CI 可显式指定 `--app-home /你的隔离绝对子目录`，布局为该目录下 `config/config.toml`、`data/shield.db`、`data/keystores/`。它不是原 GUI 数据目录的别名，不要把原数据库复制过去当成密码迁移；无效目录直接失败，不回退真实用户库。资料管理、普通 `protect` 和明确选择共享证书的 `sign` 接受它，受控 Agent／MCP 命令不会静默忽略此参数。为 `protect` 指定隔离目录默认启用共享设置，可明确用 `--use-shared-settings false` 关闭偏好读取。

## 查询和保存加固偏好

```bash
shield config show
shield config set --runtime-mode android_api19 --environment-policy strict
shield config set --sign-after-protect false
shield config set --certificate-id <证书编号>
shield config set --output-directory-mode fixed --fixed-output-directory /你的输出目录

# 清除专用选择，恢复沿用默认证书偏好；不是删除证书
shield config set --clear-certificate --inherit-auto-sign
```

只保存明确指定的字段，未指定值保持；模式使用 `standard`／`android_api19`，策略使用 `compatible`／`strict`。明确指定的证书 ID 须在当前库存在，否则不保存；不保证跨配置和数据库的原子事务，证书之后被删除时仍须重新选择。查询只返回加固业务偏好，不返回主题、统计标识、分享选择或完整原配置。坏配置／较新格式不会被默认值覆盖，未知字段和其他分区保留；注释与排版不保证保留。

这些偏好由 GUI 使用，普通 CLI 显式共享后也会读取；受控共享规划固定有效模式、策略和证书，不把之后的偏好变化带入原任务。旧脚本不自动改变行为。输入／输出仍由本次参数或项目明确提供，不自动采用 GUI 上次的输出目录／文件名。

## 证书管理

```bash
shield cert list
shield cert show <证书编号>
shield cert verify <证书编号>
shield cert edit <证书编号> --name '发布证书' --note '本地备注' --v3 true
shield cert set-default <证书编号>
shield cert clear-default
shield cert remove <证书编号> --yes
```

- 查询返回本地证书摘要，包括名称、路径、Alias 和签名偏好，但结构中没有密码。详情、偏好修改与删除记录不要求解密密码；实际校验／签名仍会明确拒绝坏密文。
- 编辑只改指定字段，材料、类型、Alias、密码不可改。修改默认项不会清除 GUI 加固页明确保存的专用选择；选择不存在的 ID 不回退其他证书。
- 删除只删记录，必须提供 `--yes`；无论外部还是托管证书文件都保留。此入口尚不提供物理文件删除。
- `verify` 会运行 keytool 并保存校验状态，失败返回错误；它不是只读命令。失败状态明确保存时附带 `change_state:confirmed` 及编号／失败事实，保存结果不明仍为 `unconfirmed`，不能将两者都当成回滚。查询不会启动 Java。

导入／创建是明确的持久化操作：

```bash
# 先由本机宿主为当前进程安全预置密码环境，再运行；不要在 Agent 对话里提供密码
shield cert import --name '已有发布证书' --keystore /你的证书目录/release.p12 \
  --keystore-type pkcs12 --alias release --set-default

shield cert create --name '新发布证书' --file-name release.p12 \
  --keystore-type pkcs12 --alias release --dname 'CN=发布证书,O=本地测试' --set-default
```

新管理命令从固定环境变量 `MOCIKA_SHIELD_KS_PASS`、可选 `MOCIKA_SHIELD_KEY_PASS` 取密码，不接受密码参数／密码配置文件，不回显环境。当前未实现终端隐藏输入提示，缺少安全的宿主注入方式时停止，不要求模型索取密码。创建时 Key 密码留空沿用 Keystore 密码，长度与 Alias 规则继续复用 GUI 共享服务。

导入默认仅记录原始文件路径，不复制或修改原文件；创建落在应用托管目录，同名只换名、不覆盖。默认 V1／V2／V3 开启，V4 关闭，自动签名偏好开启；可用 `--v1`／`--v2`／`--v3`／`--v4`／`--auto-sign` 加 `true` 或 `false` 指定。只有 `--set-default` 才设全局默认。

## 使用已保存证书独立签名

三平台预览包内 CLI 优先使用单次受控入口：

```bash
shield agent sign -i input.apk -o signed.apk --certificate-id <证书编号>
# 省略编号则使用共享全局默认证书，不要求前置计划或手工摘要
shield agent sign -i input.apk -o signed.apk
```

内部自动规划、固定材料、签名和验签，父进程无覆盖交付；首版仅 V1–V3。`agent sign-plan` 仅用于预览，不是执行前置。仅需已有共享证书和包内签名工具，不需要 apktool 或壳资源；不接受加固模式、ABI 排除或临时密码。缺资料直接返回有限错误，不修改证书偏好或交互索取密码。详细边界见[独立签名设计](../design/controlled-signing.md)。普通入口继续保留，下面的普通签名说明不代表受控入口具有相同限制：

```bash
shield sign -i input.apk -o signed.apk --certificate-id <证书编号>
shield sign -i input.apk -o signed.apk --use-default-certificate
```

不需要项目文件或再次提供密码。必须明确指定上述一种来源；暂不改变旧 `sign` 的材料默认行为。普通加固后签名见下一节。共享密码只在本机执行中解密，未设置默认项或指定 ID 失效时停止，不猜测其他记录。空 Key 密码只在本次材料快照中沿用 Keystore 密码，不重写数据库。

共享证书不得与命令、项目文件中的临时 keystore／Alias／类型，或签名密码参数／环境变量混用；包括继承的 `MOCIKA_SHIELD_KS_PASS`／`MOCIKA_SHIELD_KEY_PASS`。使用共享来源前请由宿主移除不需要的这两项变量，不改变系统全局环境。旧的按文件及环境密码签名方式保持可用。

版本优先级为：已保存证书偏好 → 项目显式值 → 本次显式值；参数缺省不覆盖证书偏好。输入／输出仍须明确提供或由项目文件指定，不自动写到 GUI 上次输出目录。沿用普通签名的对齐、输出和事件语义，不冒充 Agent 的原子无覆盖交付或取消保障；共享身份不等于授权执行。

## 使用共享偏好加固及签名

```bash
# 显式采用 GUI 保存的业务偏好；是否签名遵循保存的选择
shield protect --use-shared-settings -i input.apk -o output.apk

# 本次参数优先，不保存回默认设置
shield protect --use-shared-settings --runtime-profile android-api19 \
  --sign-after-protect false -i input.apk -o protected-unsigned.apk

# 明确选择证书即启用签名；不必读取 GUI 的其他偏好
shield protect --certificate-id <证书编号> -i input.apk -o protected-signed.apk
shield protect --use-default-certificate -i input.apk -o protected-signed.apk
```

也可在项目中明确启用（`runtime_mode` 使用下划线，与共享偏好一致）：

```toml
schema_version = 1
[protect]
use_shared_settings = true
runtime_mode = "standard"
sign_after_protect = true
use_default_certificate = true
[sign]
v1 = true
v2 = true
v3 = true
v4 = false
```

配置优先级为内置 → 已启用的共享偏好 → 项目 → 本次参数。`--use-shared-settings false` 可覆盖项目的启用选择；仅指定模式、证书或签名参数，不自动读取 GUI 的其他偏好。所有本次覆盖只影响当前任务，不写配置／证书库。

签名意图未指定时，采用全局默认证书的自动签名偏好；专用证书选择仍优先于共享默认项。继承默认项时，自动签名偏好与证书编号来自同一次查询并立即固定；之后切换默认项不会让当前任务换证书。明确关闭签名压过共享／项目设置，但本次证书选择与关闭签名不能同时指定；关闭签名后也不能传入会被忽略的签名版本参数。

签名材料作为整体选择：本次／项目证书 ID、明确默认项、显式项目临时材料，不能拼接两套来源。项目临时 keystore 可覆盖共享来源，但若本次明确选了共享 ID／默认项，又提供项目临时材料或密码环境则报错，不悄悄忽略。签名版本按证书 → 项目 → 本次 `--v1`／`--v2`／`--v3`／`--v4` 解析。

运行模式决定默认资源，显式资源也必须匹配同一模式、策略与保留 ABI；工控资源缺失不回退标准包，旧 ABI 仍须逐次明确排除，不因共享偏好自动同意。

新普通流程要求输出父目录已存在、目标 `.apk` 不存在（含链接／目录），不覆盖原文件。加固及签名先在目标目录下的私有暂存目录完成，绑定证书须一致，验签后才以同文件系统硬链接交付单 APK。失败不交付中间包；最终链接返回状态不明时保留暂存材料，须人工核对，不自动重试。该流程不等同于受控 Agent 的总期限、整组取消、材料摘要绑定或目录断电持久性，不对同权限恶意进程提供沙箱。

当前新加固后签名仅交付 V1–V3 单 APK；明确选择 V4 会拒绝，不静默关闭或只交付缺少附加文件的结果。原独立签名／GUI 的 V4 能力不受此限制影响。旧 `protect` 在未使用新参数／新项目字段时保持原行为；旧 `plan-protect` 和未选择共享来源的受控分支继续拒绝新增共享字段，不静默忽略。

## 受控共享加固

三平台预览包内 CLI 已接入共享来源；普通源码构建须显式启用 `agent-executor`，需要 MCP 时启用 `mcp-server`。先用 `agent plan --use-shared-settings` 规划，也可明确指定 `--certificate-id`／`--use-default-certificate`，输入与最终输出仍须提供。执行时使用该终态的 `plan.shared_binding` 作为 `--shared-binding`，同时提供原 `plan.sha256` 作为 `--expect-plan-sha256`；不要再次指定证书、模式或策略。隔离 `--app-home` 与本次 ABI 排除如有，须在两次调用中保持一致。

共享计划版本为 3，原文件来源版本仍为 2；信封版本不变。绑定固定证书、模式、策略及签名版本，不含密码、路径或 Alias。切换 GUI 默认项或修改无关备注不使任务换证书；选中材料或实际文件变化则停止，不回退其他来源。没有项目配置文件时也能执行，不生成明文密码配置；如显式提供项目文件，其身份、工具和有效版本仍绑定。详见[共享绑定契约](../design/shared-agent-binding.md)。

## 结果与后续接入

`cert`／`config` 成功输出单个 JSON：`schema_version: 1`、`tool_version`、`operation`、`type: result`、`status: ready`、`data`。错误为 `type: error`、固定 `code`／`message` 和 `change_state`，业务失败退出 1；语法错误退出 2 并写标准错误，帮助／版本不是 JSON。独立 `sign --json` 继续使用原逐行进度／完成／错误事件，不混用两种协议。

写操作报错可能发生在保存后的回读阶段。`change_state: unconfirmed` 表示不能确认修改结果，不表示回滚；没有收到完整响应时同样须先查询本机状态，不自动重试导入、创建或删除。查询失败不换库、不输出工具原始日志。

独立 Skill 0.3.1 包含三平台路径与调用指引，须独立安装或升级。MCP 的[共享模式](../design/shared-mcp.md)、[管理首批](../design/shared-mcp-management.md)及[生命周期](../design/mcp-certificate-lifecycle.md)已随预览包交付，复用原执行许可、共享服务及本机凭据机制；未开放的纯加固不签名、清空全局默认和输出目录持久化在接入说明中明确列出。原外置配置模式保留；宿主注册仍由用户配置，不因安装 App 自动完成。

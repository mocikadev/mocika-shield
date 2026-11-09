# 共享资料、独立签名与普通加固

只使用当前可信 CLI 已提供的入口，先按任务检查 `cert --help`、`config --help`、`sign --help` 或 `protect --help`。这些是本地资料命令，不等于 MCP 已提供相同工具；不要自行注册服务或改客户端全局设置。

## 来源与持久化

默认与 GUI 使用同一份证书、默认证书和加固偏好，GUI 无需打开。通过 CLI 查询或修改，不直接读写 `config.toml`／`shield.db`、解密字段或复制数据库。GUI 下次查询、切页或恢复焦点时可见，不承诺即时推送。

只有用户或宿主明确指定隔离上下文时，才为相关命令一致提供 `--app-home <绝对子目录>`；它会使用另一份资料，不是原 GUI 目录的别名。无效上下文不回退。不把测试隔离库说成实际用户 GUI 资料。

`cert`／`config` 不接受项目 `--config`。本次模式、策略、版本或证书参数不持久化；只有用户明确要求保存时才执行下列写操作。设置默认、清空默认及证书校验也属于写操作。

```bash
"$SHIELD_CLI" cert list
"$SHIELD_CLI" cert show "$SHIELD_CERTIFICATE_ID"
"$SHIELD_CLI" config show

# 以下均须有明确保存或管理授权，编号来自本机查询，不猜测。
"$SHIELD_CLI" config set --runtime-mode android_api19 --environment-policy strict
"$SHIELD_CLI" config set --certificate-id "$SHIELD_CERTIFICATE_ID"
"$SHIELD_CLI" config set --clear-certificate --inherit-auto-sign
"$SHIELD_CLI" cert edit "$SHIELD_CERTIFICATE_ID" --name '发布证书' --v3 true
"$SHIELD_CLI" cert set-default "$SHIELD_CERTIFICATE_ID"
"$SHIELD_CLI" cert clear-default
"$SHIELD_CLI" cert verify "$SHIELD_CERTIFICATE_ID"
```

这些变量只是本次引用，须用已确认值。查询返回没有密码的本地摘要，但其中可能有路径、Alias、备注等自由文本；只向用户回复任务所需内容，不把整个 JSON 复制进对话。库不存在时查询可能为空，不据此自动创建、导入或选择另一库。

GUI 专用证书选择和全局默认项不同：`config set --clear-certificate` 只清除加固页专用选择，`cert clear-default` 只清除全局默认；都不删除证书。编辑只能改名称、备注及签名偏好，不能更换文件、类型、Alias 或密码。

## 导入、创建与删除

导入／创建须有独立授权，并由宿主在本地安全预置 `MOCIKA_SHIELD_KS_PASS` 和可选 `MOCIKA_SHIELD_KEY_PASS`。没有安全注入方式则停止，不向用户索要对话密码，不把密码写进参数、脚本、项目或技能文件。共享库中已有材料无需重新导入或注入密码。

```bash
"$SHIELD_CLI" cert import --name '已有发布证书' --keystore "$SHIELD_KEYSTORE" \
  --keystore-type pkcs12 --alias "$SHIELD_KEY_ALIAS"
"$SHIELD_CLI" cert create --name '新发布证书' --file-name release.p12 \
  --keystore-type pkcs12 --alias release --dname 'CN=发布证书,O=本地项目'

# 仅在用户已确认具体记录的删除后使用；不删除证书文件。
"$SHIELD_CLI" cert remove "$SHIELD_CERTIFICATE_ID" --yes
```

默认导入只记原路径，创建使用应用托管目录且不覆盖同名文件；除非用户明确要求，不加 `--set-default`。材料不可变，更换材料须重新导入／创建，不能借编辑绕过。删除只移除登记，文件保留；不额外删除原件或清空数据库。

管理成功是单条 JSON：`schema_version=1`、`operation=cert|config`、`type=result`、`status=ready`。错误为 `type=error` 及固定错误码；`change_state=unconfirmed` 表示可能已修改但回读／回执不确定，不是已回滚，须先查询状态而非自动重试。语法错误退出 2，不能把 stderr 原文当安全诊断。

## 独立签名

```bash
"$SHIELD_CLI" sign --json -i "$SHIELD_INPUT" -o "$SHIELD_OUTPUT" \
  --certificate-id "$SHIELD_CERTIFICATE_ID"
# 需要全局默认项时，改用 --use-default-certificate，不同时指定编号。
```

签名需要明确授权输入、证书和输出。不要为查环境或查询证书签名，不把独立签名改成加固。不指定已存在目标或原包路径；普通入口不提供受控执行器的原子无覆盖／整组取消保证，存在其他进程同时改目标的风险时先协调，不宣称已彻底规避竞争。

共享来源禁止临时材料或外部密码环境混用；由宿主只对本次进程移除不需要的注入，不读取其值或修改系统环境。版本按证书 → 可选项目 → 本次参数解析；只按用户需求覆盖。普通 `sign --json` 使用原进度、`done`／`error` 事件，不是 `agent` 的 `result`。只有退出 0 且有完整成功终态才称完成；丢失回执时先核对输出，不重签或覆盖。普通命令不承诺受控取消，不能用只杀父进程冒充 Java 后代已停止。

## 仅加固、不签名

用户明确要求未签名产物且接受普通入口边界时，可使用：

```bash
"$SHIELD_CLI" protect --json --use-shared-settings --sign-after-protect false \
  -i "$SHIELD_INPUT" -o "$SHIELD_OUTPUT"
```

这会明确覆盖保存的自动签名意图，但不写回偏好，不同时传证书或签名版本。使用新目标；它不是 `agent plan/protect` 的受控闭环。用户要求受控取消、匹配计划后执行等保证时，说明当前仅加固入口不具备，而不是伪造计划或擅自切换签名策略。

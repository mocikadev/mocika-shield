# 平台路径与调用

适用于包含 Agent 入口的完整桌面预览包。CLI 不单独搬迁，不改包内文件；所有业务路径在包外。宿主与 CLI 使用同一系统和用户环境，WSL、容器及远程进程不自动复用原生 GUI 的资料。

| 安装方式 | CLI | 同包资源 |
|---|---|---|
| macOS | 实际 `.app/Contents/MacOS/shield` | `Contents/Resources/{tools,resources}` |
| Linux deb | `/usr/bin/mocika-shield-cli` | `/usr/lib/MocikaShield/{tools,resources}` |
| Linux AppImage | 完整解包目录的 `usr/bin/mocika-shield-cli` | 同一解包目录的 `usr/lib/MocikaShield/{tools,resources}` |
| Windows | 实际安装目录的 `mocika-shield-cli.exe` | 同目录 `{tools,resources}` |

安装别名不会被 Bash 脚本的 PATH `shield` 搜索识别，优先使用本次明确路径或宿主 `MOCIKA_SHIELD_CLI`。AppImage 不使用临时挂载路径；未准备稳定解包目录时请用户准备，不自行安装、解包或升级应用。宿主需要完整 JDK 8+，但缺 Java 不等于授权安装或修改全局环境。

## 路径定位

遵循“本次明确绝对路径 → 宿主专用环境变量 → PATH 中的 `shield` → 询问”。显式路径或已设置变量为空、无效时停止，不回退；拒绝换行和非文件路径，多候选不猜测。定位期间不运行候选、不联网、不读取其他环境或遍历用户目录。

macOS／Linux 可从技能根目录通过参数数组调用 `/bin/bash <技能根目录>/scripts/resolve-cli.sh [本次绝对路径]`。脚本只返回候选，退出码和来源检查见[命令参考](cli.md)。

Windows 不调用 `.sh`，不要求 Git Bash 或 WSL。由宿主本机路径接口或 PowerShell `Test-Path -LiteralPath ... -PathType Leaf` 检查明确的盘符／UNC 绝对路径，保留空格和中文，不去掉反斜杠、不解释通配符。没有显式路径或专用变量时，可用 `Get-Command shield.exe -CommandType Application -All` 查 PATH；无结果询问用户，不把同名函数、别名或 `.cmd`／`.bat` 当作候选，不递归寻找安装位置。多候选只有确认是同一文件时才合并，否则要求明确选择。

## 调用语法

优先使用宿主进程接口：可执行路径独立传入，每个参数是独立数组元素，不拼 shell 命令、不用 `eval`／`Invoke-Expression`。确认可信后，先核对 `--version`、本次命令 `--help` 及所需协议。

macOS／Linux 的 Bash 示例：

```bash
"$SHIELD_CLI" agent sign -i "$SHIELD_INPUT" -o "$SHIELD_OUTPUT" \
  --certificate-id "$SHIELD_CERTIFICATE_ID"
```

Windows 的 PowerShell 等价调用：

```powershell
& $ShieldCli agent sign -i $ShieldInput -o $ShieldOutput --certificate-id $ShieldCertificateId
```

变量必须来自本次已确认值。其他参考中的 Bash 参数同样可用 PowerShell 的 `&` 调用，但不能照抄 Bash 的反斜杠续行、`export` 或 Unix 信号。输出按 UTF-8 解码；证书列表仅在本机解析，不向对话转述整份资料。

## 生命周期与升级

需要跨平台取消且宿主能持续持有标准输入时，在启动本次受控任务时加 `--control-stdin`，取消时写一行 `{"type":"cancel"}`，持续读取终态。EOF 也会取消，不能启动后立即关闭标准输入。macOS／Linux 可向本次父进程发送 SIGINT／SIGTERM；Windows 控制台 Ctrl+C／Break 仅适用于宿主确实能定向传递的控制台，不能将 `Stop-Process` 当作已确认的正常取消。无可靠生命周期控制时只规划。

App 升级后重新检查 CLI 路径和能力，路径变化只更新宿主本地配置；不改 Skill 保存机器路径。更新 Skill 时保留整目录和宿主配置，不跨升级复用计划。不因资料不可见就复制数据库或改用户上下文。

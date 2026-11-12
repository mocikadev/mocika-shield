# 发布与版本管理

## 版本与预发布

| 类型 | 用途 |
|---|---|
| Major | 对用户接口造成破坏性变化，或明确的重大里程碑 |
| Minor | 一组完整、面向用户的新能力，不按每个小功能机械升级 |
| Patch | 缺陷与兼容修复 |
| Alpha | 能力探索，允许调整方案和接口 |
| Beta | 主要功能完成，冻结新增范围，重点验证和修复 |
| RC | 发布候选，只修发布阻塞问题 |
| 正式版 | 已批准范围和必要门禁通过，剩余限制在说明中明确 |

不要求每次都走完所有预发布阶段，也不为仅改版本号连续发包。标签统一为小写 `v` 加 SemVer，如 `v1.5.0-beta.1`。

DEXB 或壳协议变化不自动等于 Major：已生成 APK 自包含运行时，但工具／资源配对及重新加固要求仍须说明，不能宣称格式变化对用户完全无感。

## 分支与 PR

采用 `main` 加临时 `feat/*`、`fix/*`、`docs/*` 分支，合并后清理。不默认增加长期 `develop`；确需并行维护稳定线时，从对应稳定标签建立维护分支。

### main 分支保护规则

当前仓库要求 PR、分支与最新主干同步、解决全部对话及 `基础快速检查` 通过；必需人工审批为 0。禁止强推或删除 main，暂不要求签名提交。管理员强制执行目前未启用，但不作为日常绕过规则的许可。

日常流程：从最新主干建临时分支，完成整个阶段及对应验证，集中提交和提审，正常合并后删除任务分支。不要直接推送 main，也不要为小步调试积累大量 PR。

## CI 分工

| 工作流 | 范围 |
|---|---|
| `ci.yml` | 普通 PR／主干提交按变更范围运行快速检查；纯文档可跳过代码检查 |
| `ci.yml` 手动 | 默认 `all` 完整验证，也可选择 Windows 资源、Agent 原生／实际安装包、进程或 Java 等专项 |
| `release.yml` | 标签或手动触发，完整构建 Linux、macOS Universal、Windows 及发布汇总 |
| `project-stats.yml` | 每日或手动更新维护统计，不是发布前门禁 |

快速检查含 Rust 格式、轻量 Linux 启动策略及脚本测试，不编译整套 GUI／Android 壳。重型任务按影响手动运行或随发布执行，不给每次提交增加等待。

```bash
gh workflow run ci.yml --ref <待验证分支> -f validation_scope=windows_resources
```

其他专项值及证据见[平台验收表](agent-desktop-platforms-plan.md)，不把一次完整 CI 当作全部设备业务已经通过。

## 同步版本

优先执行：

```bash
bash scripts/bump-version.sh x.y.z
# 或 x.y.z-beta.1、x.y.z-rc.1
```

脚本同步核心、共享服务、CLI、GUI 后端／Tauri／前端及标准／API 19 Stub 的版本。API 19 独立 Cargo.lock 使用 Rust 1.77.2 离线更新；根 Cargo.lock 也须核对。禁止只改安装包名或遗漏 `shield-services`／兼容锁文件。

## 发布步骤

1. 在临时分支同步版本、版本系列说明和受影响文档，完成对应本地／专项验证。
2. 合并 PR，确认主干检查完成、工作区干净，并运行 `bash scripts/check-release-ready.sh`。
3. 经维护者确认发布后，从已验证主干创建新标签；只推标签，不借发布命令直接推 main。
4. 等待三个平台和汇总任务成功，核对产物、校验和、内置资源及说明。
5. 正式版保持草稿供验收，确认后公开；预发布工作流直接公开为 Pre-release，打标签前须明确该可见性。

```bash
git switch main
git pull --ff-only
git tag vx.y.z
git push origin vx.y.z
```

示例中的版本必须替换为本次已批准值。手动运行 Release 工作流不会自动创建 Git 标签，必须核对所选分支、输入版本和目标提交一致。

公开标签不再移动。未公开、尚无对外产物的失败构建，只有取得维护者对具体标签的明确授权后才能更新并重建；不能将此当作默认重试方式。

## 产物与打包约束

| 平台 | Release 文件 |
|---|---|
| Linux x64 | `MocikaShield_X.Y.Z_linux_amd64.AppImage`、`.deb`、`linux-tauri-checksums-sha256.txt` |
| macOS Universal | `MocikaShield_X.Y.Z_macos_universal.dmg`、`macos-tauri-checksums-sha256.txt` |
| Windows x64 | `MocikaShield_X.Y.Z_windows_x64_setup.exe`、`windows-checksums-sha256.txt` |

共四个安装包、三份校验和，均发布到 `mocikadev/mocika-shield`。校验和记录使用扁平文件名，拒绝重复或无效记录。

从 1.5.0 预览版起 CLI／MCP 随 GUI 共包；不新增独立 CLI Release 资产。Skill 独立安装升级，不进入 App，也不是 App 构建前置材料。路径见[接入说明](../ops/agent-setup.md)。

本地平台脚本和命令只在[构建指南](../ops/build.md#安装包构建)维护。`SKIP_CLI_RELEASE=1` 只跳过维护用独立 CLI 压缩包，不省略 GUI 包内 CLI。

打包必须在双壳生成后检查 `metadata.json.version` 与本次完整版本一致，含预发布后缀；共同检查入口为 `scripts/check_resource_version.py`。辅助 JAR 也须与当前构建匹配。macOS 先装入 CLI，再做内层和整包签名，不能事后复制文件冒充完整包。

### 正式版前产物检查

- 核对安装包及 CLI／GUI 版本、架构、摘要和标准／工控资源契约。
- 内置工具含 apktool、apksigner、java-adapter JAR；正式资源含标准和工控双 ZIP。
- 不含测试 APK、证书、数据库、配置、环境文件、缓存或 `.DS_Store`。
- 说明包含 Java 8+、平台要求、签名一致性、支持边界及 macOS 未公证提示。
- 按变更范围抽查真实安装包；沿用未受影响的有效回归，不机械重复全系统／全业务矩阵。实际执行与沿用证据分别记录。
- 验收结果至少写明版本／来源、平台／Java、样本类型、通过项及未验范围；不将条件忽略或静态检查算作实际运行。

完整测试范围见[清单](test-checklist.md)。官方包验收不等于所有用户设备、Agent 客户端或发行版均已验证。

## Release Notes

| 文件 | 作用 |
|---|---|
| `.github/release.yml` | GitHub 自动变更分类 |
| `.github/release-notes/stable.md`、`prerelease.md` | 简洁中英文前言 |
| `.github/release-notes/versions/x.y.z.md` | 同一版本系列的能力汇总，Alpha／Beta／RC／正式版共用 |

固定章节按顺序保留：`下载 / Downloads`、`使用须知 / Notes`、`本次变更 / What's Changed`，末尾保留 `Full Changelog` 比较链接。

自动变更以上一个已公开正式版本为基线，覆盖整个开发周期，不只比较相邻预发布版。合并列表时移除 GitHub 重复标题。人工精简可整理成少量中英文亮点，但不能删除固定章节或重复整份 README。

重新运行相同标签的构建会重写产物和说明；须重新验收，公开版本不得以此悄悄替换。可用 `gh release view <tag> --json body --jq '.body'` 复核正文。

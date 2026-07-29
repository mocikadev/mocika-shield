# 发布与版本管理

## 版本号语义

稳定版本号格式：`major.minor.patch`（如 `1.2.3`）。预发布版本允许 SemVer 后缀，例如 `1.2.0-rc.1`。

| 版本位 | 触发条件 | 示例 |
|--------|----------|------|
| **Major** | 功能积累到一定程度的里程碑版本，或 CLI/GUI 接口有破坏性变更 | 多个 Minor 功能积累后升为 2.0；CLI 子命令结构重构等 |
| **Minor** | 新增一个完整功能 | 版本更新提示、反调试检测、CLI 子命令等，每个对应一次 minor 升级 |
| **Patch** | Bug 修复，不新增功能 | 签名检测修复、异常路径 fail-closed 修复等 |

> DEXB 格式变化属于内部算法优化，**不触发 major 升级**。由于加固后的 APK 自包含壳模块，格式升级对用户完全无感。

### Git Tag 命名规范

发布时统一使用 `v{version}` 格式，如 `v1.0.0`、`v1.2.3`、`v1.2.0-rc.1`。

- 前缀固定小写 `v`
- 不使用 `V`（大写）、不省略前缀
- 只允许标准 SemVer 预发布后缀（如 `-rc.1`），不使用非标准后缀（如 `-stable`）
- GUI 版本检查解析时兼容大小写（`v`/`V` 均可 strip），但发布时只用小写

### 分支管理策略

当前阶段采用 **单主干 `main` + tag 发布**：

- `main`：长期稳定主干，保持可构建、可发布
- `vX.Y.Z` / `vX.Y.Z-rc.N`：唯一正式发布标记
- `feat/*`：复杂功能的临时开发分支，合并回 `main` 后删除
- `fix/*`：缺陷修复的临时分支，合并回 `main` 后删除

暂不维护长期 `develop` 分支，也不默认创建 `release/*` / `hotfix/*` 分支，避免刚开源阶段增加流程成本。

只有出现以下情况时，才新增长期维护分支：

- 多个正式版本线需要并行维护，例如 `1.2.x` 与 `1.3.x`
- 某个大功能周期较长，不能持续保持 `main` 可发布
- 多人协作规模扩大，需要隔离稳定分支和开发分支

如需维护旧版补丁，优先从对应稳定 tag 拉出 `release/x.y`，修复后打 `vX.Y.Z` patch tag。

#### `main` 分支保护规则

GitHub 的 `main` 分支必须保持保护状态，正式代码统一通过临时分支和 Pull Request 合入。当前规则按单维护者仓库配置，不要求作者无法自行完成的人工审批。

| 规则 | 配置 |
|------|------|
| 合并前必须创建 Pull Request | 启用 |
| 必需审批数 | `0` |
| 合并前必须更新到最新 `main` | 启用 |
| 必须解决全部对话 | 启用 |
| 允许强制推送 | 禁止 |
| 允许删除 `main` | 禁止 |
| 要求签名提交 | 暂不启用 |
| 管理员强制执行 | 暂不启用，保留紧急绕过能力 |

Pull Request 合并前必须通过以下 CI 检查：

- `基础快速检查`

普通 PR 只执行 Rust 格式和脚本契约测试，避免每次提交重复等待完整编译与跨平台打包。完整代码质量检查、Android 壳构建、Linux Tauri 打包冒烟检查、Windows Android 4.4 资源构建和发布前检查仅在手动触发 CI 时执行；版本发布仍由 Release 工作流完整构建三个平台。

日常开发流程：

1. 从最新 `main` 创建 `feat/*`、`fix/*`、`docs/*` 等临时分支。
2. 完成修改和本地验证后推送远端并创建 Pull Request。
3. 等待全部必需 CI 通过，并解决未完成的评审对话。
4. 合并到 `main`，随后删除临时分支。
5. 只有发布版本时才从已验证的 `main` 创建并推送 `vX.Y.Z` tag。

管理员绕过只用于保护规则配置错误、CI 基础设施不可用或紧急安全修复。绕过后必须补建对应 Pull Request 或维护记录，不作为日常直接推送 `main` 的方式。

### GUI 版本更新提示策略

| 检测到版本差异 | 提示方式 |
|----------------|----------|
| **Patch** | 顶部小提示条，可一键关闭 |
| **Minor** | 顶部提示条，持续显示直到用户手动关闭 |
| **Major** | 启动时弹窗，突出"重大版本更新"，引导用户前往 Release 页查看变更说明 |

---

## 版本号同步

升级版本时，需同步修改以下文件：

| 文件 | 字段 |
|------|------|
| `crates/shield-core/Cargo.toml` | `version = "x.y.z[-pre]"` |
| `apps/shield-cli/Cargo.toml` | `version = "x.y.z[-pre]"` |
| `shield-stub/src/main/rust/Cargo.toml` | `version = "x.y.z[-pre]"` |
| `shield-stub/compat/api19-rust/Cargo.toml` | `version = "x.y.z[-pre]"` |
| `shield-stub/compat/api19-rust/Cargo.lock` | 独立兼容 crate 的根包版本；发布构建使用 `--locked` |
| `apps/shield-gui/src-tauri/Cargo.toml` | `version = "x.y.z[-pre]"` |
| `apps/shield-gui/src-tauri/tauri.conf.json` | `"version": "x.y.z[-pre]"` |
| `apps/shield-gui/package.json` | `"version": "x.y.z[-pre]"` |
| `apps/shield-gui/package-lock.json` | `"version": "x.y.z[-pre]"` |

优先使用：

```bash
bash scripts/bump-version.sh x.y.z
bash scripts/bump-version.sh x.y.z-rc.1
```

脚本会使用 Rust 1.77.2 离线同步 API 19 兼容 crate 的独立锁文件；修改后仍需运行 `cargo build` 或 `make build-all`，使根 `Cargo.lock` 同步更新。

---

## 发布命令

### Linux / macOS

```bash
# CLI-only 发布包（维护者本地使用）
make release VERSION=x.y.z

# 仅 CLI 发布包（tar.gz，本地生成，不由 GitHub Release 上传）
bash scripts/release-cli.sh x.y.z

# Linux 本地发布（默认 GUI + CLI；CI 设置 SKIP_CLI_RELEASE=1 只上传 GUI）
VERSION=x.y.z make release-linux

# macOS 本地发布（默认 GUI + CLI；CI 设置 SKIP_CLI_RELEASE=1 只上传 GUI）
VERSION=x.y.z make release-macos

```

### Windows（必须在 Windows 原生环境执行）

```powershell
# Windows 本地发布（默认 GUI + CLI；CI 设置 SKIP_CLI_RELEASE=1 只上传 GUI）
.\scripts\release-windows.ps1 -Version x.y.z
```

脚本会自动检测并安装缺失的 cargo 工具（tauri-cli / cargo-ndk），并通过 npm 构建 React 前端，首次运行耗时较长。

---

## 发布包结构

本地发布脚本默认仍会生成 CLI 与 GUI 产物，便于维护者离线分发或调试；GitHub Actions 的 Release workflow 会设置 `SKIP_CLI_RELEASE=1`，只构建并上传 GUI 安装包。

### CLI 本地发布包

```
mocika-shield-x.y.z/
├── bin/
│   └── shield              # 可执行文件（Windows 为 shield.exe）
├── lib/
│   ├── apktool.jar
│   └── apksigner.jar
├── resources/
│   └── resources.zip       # shield-stub 产物（Android 壳 DEX + .so）
└── README.md
```

### GUI 发布包

| 平台 | 产物 | 说明 |
|------|------|------|
| Linux（Tauri） | `MocikaShield_x.y.z_linux_amd64.AppImage` | 免安装，直接运行 |
| Linux（Tauri） | `MocikaShield_x.y.z_linux_amd64.deb` | Debian/Ubuntu 安装包 |
| macOS（Tauri） | `MocikaShield_x.y.z_macos_aarch64.dmg` / `MocikaShield_x.y.z_macos_universal.dmg` | Tauri 版 |
| Windows | `MocikaShield_x.y.z_windows_x64_setup.exe` | NSIS 安装包 |

GUI 发布包已内置 apktool.jar、apksigner.jar、resources.zip，用户无需额外配置工具路径。

### Windows 本地发布产物（`dist/` 目录）

```
dist/windows/
├── MocikaShield_x.y.z_windows_x64_setup.exe    # GUI NSIS 安装包
├── mocika-shield-cli-x.y.z-windows-x86_64.zip  # CLI 本地发布包；CI 不上传
└── checksums-sha256.txt
```

---

## 构建顺序约束

`shield`（CLI）和 GUI 运行时均依赖 `resources.zip`，**必须先完成 shield-stub 构建**：

```
make build-stub  →  make build-cli / make build-gui / make release / make release-linux / make release-macos
```

`make release` 是 CLI-only 本地发布包；各平台 GUI 发布脚本均已内置必要构建顺序，无需手动保证。

---

## GitHub Actions CI/CD

仓库包含两个工作流：

| 工作流 | 文件 | 触发 | 内容 |
|--------|------|------|------|
| CI | `.github/workflows/ci.yml` | push / pull request / 手动触发 | 普通提交与 PR 执行基础快速检查；手动触发时执行完整代码质量、Android 壳、Linux Tauri、Windows Android 4.4 资源及发布前检查 |
| Release | `.github/workflows/release.yml` | tag `v*.*.*` / 手动触发 | 并行构建 Linux Tauri、macOS Tauri、Windows GUI 产物，汇总上传到 GitHub Release |

> GitHub Release 面向普通开源用户，只上传 GUI 安装包与校验和；CLI 包仍可通过本地发布脚本生成，但不会由 CI 上传到 Release。

Release Notes 相关文件：

| 文件 | 作用 |
|------|------|
| `.github/release.yml` | GitHub 自动生成变更列表的分类配置 |
| `.github/release-notes/stable.md` | 稳定版本固定前言模板 |
| `.github/release-notes/prerelease.md` | 预发布版本固定前言模板 |

### 自动发布流程

```bash
# 1. 更新版本号并提交
make bump-version V=x.y.z
git add .
git commit -m "chore: 发布 x.y.z"

# 2. 打 tag 并推送
git tag vx.y.z
git push origin main vx.y.z
```

推送 tag 后，`Release` workflow 会自动：

1. 从 tag 提取版本号
2. 构建各平台 GUI 产物
3. 上传 workflow artifacts
4. 根据版本号创建或更新对应的 GitHub Release
5. 上传 GUI 安装包和校验和文件

各平台发布脚本生成的校验和保留本地 `dist` 子目录，便于维护者直接校验本地产物。Release 汇总任务上传前会将记录规范化为扁平文件名，并拒绝无效记录或重复文件名，确保下载校验和文件后可在安装包所在目录直接执行校验。

发布可见性规则：

- **稳定版本**（如 `v1.2.0`）：自动创建为 **Draft**
- **预发布版本**（如 `v1.2.0-rc.1`、`v1.2.0-beta.1`、`v1.2.0-alpha.1`）：自动创建为 **Pre-release**

稳定版本继续保留人工验收窗口；预发布版本直接公开为候选版本，避免每次手动从 Draft 改为 Pre-release。

Release Notes 生成规则：

- workflow 先读取稳定版或预发布版的简洁中英文前言模板
- 再调用 GitHub Release Notes API 生成本次版本的自动变更列表
- 最终将两部分合并后写入 Release
- 如果重新运行同一个 tag 的发布任务，产物与 Release Notes 会一并更新

所有稳定版和预发布版 Release Notes 固定保留以下章节，顺序保持一致：

1. `下载 / Downloads`
2. `使用须知 / Notes`
3. `本次变更 / What's Changed`
4. `完整变更 / Full Changelog`

Release Notes 只保留下载入口、必要运行条件、安全边界和本版本变更，不重复 README 中的完整功能介绍。人工精简发布说明时，可以把自动列表整理成 2～5 条中英文版本亮点，但必须放在 `本次变更 / What's Changed` 下，不得改名或删除固定章节。编辑完成后使用 `gh release view <tag> --json body --jq '.body'` 复核章节；若之后重新运行同一个 tag 的发布任务，自动生成内容会覆盖人工修改，需要再次检查固定章节和版本亮点。

### 手动触发发布

在 GitHub Actions 页面选择 `Release` workflow，输入版本号 `x.y.z` 后运行。手动触发不会自动创建 git tag；正式发布仍建议使用 tag 触发。
如果输入的是稳定版本号，会创建或更新 `vx.y.z` Draft Release；如果输入的是带预发布后缀的版本号（如 `1.2.0-rc.1`），会直接创建或更新为 Pre-release。

---

## 发布检查清单

1. 更新版本号（见上方"版本号同步"）
2. 运行发布前轻量检查：`bash scripts/check-release-ready.sh`
3. 确认 CI 通过
4. 打 tag 并推送：`git tag vx.y.z && git push origin main vx.y.z`
5. 等待 Release workflow 完成
6. 检查 GitHub Release 的产物、校验和，以及固定的中英文 Release Notes 章节
7. 稳定版本确认无误后取消草稿正式发布；预发布版本确认可见性与产物即可

### 正式版前产物检查

正式版发布前需额外确认：

- Release 页面只包含 GUI 安装包与校验和文件，不上传 CLI 包
- 安装包内包含 `apktool.jar`、`apksigner.jar`、`resources.zip`
- 安装包内不包含测试 APK、测试证书、`shield.db`、`config.toml`、`.env` 或本地缓存
- README、使用文档、Release Notes 已说明 Java 8+、证书管理、密码加密、16 KB 对齐和 macOS 未签名提示
- 支持与问题反馈文档、issue 模板和关于页诊断信息入口保持一致
- 下载 Release 产物后至少完成一次证书导入/创建、设为默认、签名、加固、自动签名回归
- 解压内置 `resources.zip` 与 `resources-api19.zip`，确认只包含预期 DEX、元数据和 Native 库，不包含 `.DS_Store`、测试文件或其他本机临时产物

`1.2.7` 的候选版本、设备矩阵、未决事项和正式版判断记录在[测试清单的发布前收尾审计](test-checklist.md#2026-07-29127-发布前收尾审计)中。

### 产物与命名规则

| 平台 | 产物文件 |
|------|----------|
| Linux（Tauri） | `MocikaShield_X.Y.Z_linux_amd64.AppImage`、`MocikaShield_X.Y.Z_linux_amd64.deb`、`linux-tauri-checksums-sha256.txt` |
| macOS（Tauri） | `MocikaShield_X.Y.Z_macos_universal.dmg`、`macos-tauri-checksums-sha256.txt` |
| Windows | `MocikaShield_X.Y.Z_windows_x64_setup.exe`、`windows-checksums-sha256.txt` |

> 发布仓库：`mocikadev/mocika-shield`（源码与 Release 包同仓库维护）

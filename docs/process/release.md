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
| `shield-cli/Cargo.toml` | `version = "x.y.z[-pre]"` |
| `shield-stub/src/main/rust/Cargo.toml` | `version = "x.y.z[-pre]"` |
| `shield-gui/src-tauri/Cargo.toml` | `version = "x.y.z[-pre]"` |
| `shield-gui/src-tauri/tauri.conf.json` | `"version": "x.y.z[-pre]"` |
| `shield-gui/package.json` | `"version": "x.y.z[-pre]"` |
| `shield-gui/package-lock.json` | `"version": "x.y.z[-pre]"` |

优先使用：

```bash
bash scripts/bump-version.sh x.y.z
bash scripts/bump-version.sh x.y.z-rc.1
```

修改后需重新运行 `cargo build` 或 `make build-all` 使 `Cargo.lock` 同步更新。

---

## 发布命令

### Linux / macOS

```bash
# 旧版 CLI-only 发布包（向后兼容）
make release VERSION=x.y.z

# 仅 CLI 发布包（tar.gz）
bash scripts/release-cli.sh x.y.z

# Linux 全量发布（AppImage + deb + CLI tar.gz）
VERSION=x.y.z make release-linux

# macOS Tauri GUI 全量发布（dmg + CLI tar.gz）
VERSION=x.y.z make release-macos

```

### Windows（必须在 Windows 原生环境执行）

```powershell
# 完整发布包（build-all + NSIS 安装包 + CLI zip + 校验和）
.\scripts\release-windows.ps1 -Version x.y.z
```

脚本会自动检测并安装缺失的 cargo 工具（tauri-cli / cargo-ndk），并通过 npm 构建 React 前端，首次运行耗时较长。

---

## 发布包结构

### CLI 发布包（`mocika-shield-x.y.z.tar.gz`）

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

### Windows 发布产物（`dist/` 目录）

```
dist/windows/
├── MocikaShield_x.y.z_windows_x64_setup.exe    # GUI NSIS 安装包
├── MocikaShield_x.y.z_windows_x64_setup.exe.sha256
├── mocika-shield-cli-x.y.z-windows-x86_64.zip  # CLI 发布包
└── mocika-shield-cli-x.y.z-windows-x86_64.zip.sha256
```

---

## 构建顺序约束

`shield`（CLI）和 GUI 运行时均依赖 `resources.zip`，**必须先完成 shield-stub 构建**：

```
make build-stub  →  make build-cli / make build-gui / make release / make release-linux / make release-macos
```

`make release` 是旧版 CLI-only 发布包；各平台 GUI 发布脚本均已内置必要构建顺序，无需手动保证。

---

## GitHub Actions CI/CD

仓库包含两个工作流：

| 工作流 | 文件 | 触发 | 内容 |
|--------|------|------|------|
| CI | `.github/workflows/ci.yml` | push / pull request / 手动触发 | Rust 格式检查、CLI 单元测试、stub Rust 单元测试、Android 壳构建、Tauri GUI 检查 |
| Release | `.github/workflows/release.yml` | tag `v*.*.*` / 手动触发 | 并行构建 Linux Tauri、macOS Tauri、Windows 产物，汇总上传到 GitHub Release 草稿 |

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
2. 构建各平台产物
3. 上传 workflow artifacts
4. 创建或更新 `vX.Y.Z` GitHub Release 草稿
5. 上传所有构建产物和校验和文件

Release 默认创建为草稿，检查产物和 Release Notes 后再手动发布。

### 手动触发发布

在 GitHub Actions 页面选择 `Release` workflow，输入版本号 `x.y.z` 后运行。手动触发会创建或更新 `vx.y.z` Release 草稿，但不会自动创建 git tag；正式发布仍建议使用 tag 触发。

---

## 发布检查清单

1. 更新版本号（见上方"版本号同步"）
2. 确认 CI 通过
3. 打 tag 并推送：`git tag vx.y.z && git push origin main vx.y.z`
4. 等待 Release workflow 完成
5. 检查 GitHub Release 草稿的产物、校验和和 Release Notes
6. 确认无误后取消草稿正式发布

### 产物与命名规则

| 平台 | 产物文件 |
|------|----------|
| Linux（Tauri） | `MocikaShield_X.Y.Z_linux_amd64.AppImage`、`MocikaShield_X.Y.Z_linux_amd64.deb`、CLI tar.gz |
| macOS（Tauri） | `MocikaShield_X.Y.Z_macos_universal.dmg` + `.sha256` |
| Windows | `MocikaShield_X.Y.Z_windows_x64_setup.exe`、CLI zip |

> 发布仓库：`mocikadev/mocika-shield`（源码与 Release 包同仓库维护）

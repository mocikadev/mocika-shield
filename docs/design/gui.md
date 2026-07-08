# GUI 设计文档

> 当前唯一正式桌面 GUI：`shield-gui`（Tauri v2 + React + TypeScript）
> 最后更新：2026-07-07

## 1. 定位

`shield-gui` 是面向桌面环境的 APK 加固工具界面，目标是把“选择 APK、执行加固、自动签名、查看结果”这条链路收敛到一个稳定的跨平台应用里。

维护约束只有两条：

- 正式版本只维护这一套 Tauri GUI，不再保留第二套桌面实现
- GUI 后端继续直接链接 `shield-cli` 库，不通过额外子进程包装业务逻辑

## 2. 技术栈

| 层 | 当前实现 |
|----|----------|
| 桌面容器 | Tauri v2 |
| 后端 | Rust，`shield-gui/src-tauri/src/main.rs` |
| 前端 | React + TypeScript |
| 构建 | Vite |
| 样式 | Tailwind CSS |
| 组件 | `src/components/ui/*`（基于 shadcn/ui 体系整理） |
| 图标 | lucide-react |

## 3. 信息架构

GUI 固定四个页面：

| 页面 | 作用 |
|------|------|
| 加固 | 预检 APK、执行加固、可选自动签名、展示输出结果 |
| 签名 | 对 APK 单独签名，只使用设置页保存的正式配置 |
| 设置 | 主题、语言、签名配置、签名版本 |
| 关于 | 版本号、构建信息、工具状态、检查更新 |

导航约束：

- 左侧侧边栏只放主入口，不在页面内部重复做二级导航
- 加固、签名属于工作流入口，设置、关于属于应用级入口
- 侧边栏收起后只保留图标，页面顺序和位置不变

## 4. 交互原则

- 默认先展示工作流入口，不把设置页当首页
- 文件一旦选中，页面进入“已选择 APK”状态，不继续显示空拖拽态
- 预检失败要明确指出原因，例如“不是 APK”“已加固”“未签名”
- 长路径、错误信息和技术日志允许换行，但按钮文案不换行
- 错误信息优先展示摘要，技术详情支持复制
- 主操作按钮位置稳定，加固进度更新时不应导致页面跳动

## 5. 签名配置规则

签名配置只有一份，来源固定为设置页保存的正式配置。

| 项目 | 规则 |
|------|------|
| 配置来源 | 设置页 |
| 配置文件 | `config.toml` |
| 签名页 | 不维护临时配置，只读取设置页保存值 |
| 自动签名 | 加固完成后复用同一份配置 |
| Keystore 类型 | 根据扩展名自动推断，可手动覆盖 |
| Alias | 支持手动输入与 `keytool -list` 自动识别 |
| 密码 | 本地明文存储，需要在文档中明确风险 |

当前默认签名版本：`V1 + V2 + V3`，`V4` 默认关闭。

## 6. 核心工作流

### 6.1 加固

```text
选择 APK
  → 预检文件类型、签名状态、是否已加固
  → 检查 Java / apktool / resources.zip
  → 调用 shield-cli::protect_apk
  → 接收分步进度事件
  → 可选自动签名
  → 输出 protected.apk 或 protected_signed.apk
```

对应进度步骤：

| key | 说明 |
|-----|------|
| `CheckTools` | 检查工具 |
| `Unpack` | 解包 APK |
| `ModifyManifest` | 修改 Manifest |
| `ProcessDex` | 处理 DEX |
| `InjectRuntime` | 注入 Runtime |
| `Repack` | 重打包 |
| `Sign` | 自动签名 |

### 6.2 独立签名

```text
选择 APK
  → 读取设置页中的正式签名配置
  → 必要时做证书指纹对比
  → 调用 shield-cli::sign_apk
  → 输出 signed.apk
```

指纹不一致时给出警告，但是否继续由用户决定。

## 7. 后端集成

Tauri command 和 `shield-cli` 的关系如下：

```text
protect_apk
  → spawn_blocking
  → shield_cli::protect_apk(...)
  → emit("protect-progress")

sign_apk
  → spawn_blocking
  → shield_cli::sign_apk(...)

check_apk / check_update / load_sign_config / save_sign_config
  → 统一在 src-tauri/src/main.rs 中维护
```

Windows 下调用 `java`、`keytool`、`apksigner` 等子进程时，必须统一走 `no_window_command()`，避免弹出控制台窗口。

## 8. 配置与持久化

GUI 自动维护的配置文件固定为 `config.toml`，应用启动时载入一次，后续页面共享同一份内存状态：

| 平台 | 路径 |
|------|------|
| Linux | `~/.config/dev.mocika.shield-gui/config.toml` |
| macOS | `~/Library/Application Support/dev.mocika.shield-gui/config.toml` |
| Windows | `%APPDATA%\\dev.mocika.shield-gui\\config.toml` |

命名约束：

- `config.toml`：GUI 自动读写的唯一正式配置
- `mocika-shield.toml`：如未来为 CLI 增加人工配置文件，可使用独立文件名
- 旧版 `tool_config.json` 在新版本首次启动时自动迁移并清理

## 9. 更新检查

GUI 启动时会调用 GitHub Releases API 检查版本更新，并在关于页提供手动检查入口。

规则如下：

- `patch` / `minor`：顶部提示条
- `major`：弹窗提示
- 忽略状态写入 `dismissed_version`
- 版本比较按 SemVer 处理，支持 `1.2.0-rc.1` 这类预发布版本

## 10. 维护要求

- README、使用指南、发布文档只描述当前这套 GUI
- 新功能优先补齐中英文文案、深浅色主题和窄窗口状态
- 文档里不再保留历史 GUI 的正式入口

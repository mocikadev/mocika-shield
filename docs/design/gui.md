# GUI 设计文档

> 当前唯一正式桌面 GUI：`apps/shield-gui`（Tauri v2 + React + TypeScript）
> 最后更新：2026-07-27

## 1. 定位

`apps/shield-gui` 是面向桌面环境的 APK 加固工具界面，目标是把“选择 APK、执行加固、自动签名、查看结果”这条链路收敛到一个稳定的跨平台应用里。

维护约束只有两条：

- 正式版本只维护这一套 Tauri GUI，不再保留第二套桌面实现
- GUI 后端继续直接链接 `shield-core` 库，不通过额外子进程包装业务逻辑
- 标准模式与 Android 4.4 兼容模式使用固定枚举映射到应用内置资源，前端不得提交任意资源路径；缺省值保持标准模式

## 2. 技术栈

| 层 | 当前实现 |
|----|----------|
| 桌面容器 | Tauri v2 |
| 后端 | Rust，`main.rs` + 模块化后端（`app_config.rs`、`signing.rs`、`updates.rs` 等） |
| 前端 | React + TypeScript，`App.tsx` 只保留应用壳，页面拆到 `pages/*` |
| 构建 | Vite |
| 样式 | Tailwind CSS |
| 组件 | `src/components/ui/*` + `src/components/app/*`（基于 shadcn/ui 体系整理） |
| 页面逻辑 | 页面内部副作用优先抽到 `src/hooks/*`，布局块优先收敛到 `src/components/app/*` |
| 图标 | lucide-react |

## 3. 信息架构

GUI 目标固定五个页面：

| 页面 | 作用 |
|------|------|
| 加固 | 预检 APK、执行加固、可选自动签名、展示输出结果 |
| 签名 | 对 APK 单独签名，直接选择已保存的证书 |
| 证书 | 统一管理签名证书：创建、导入、编辑、设默认、删除 |
| 设置 | 主题、语言、更新偏好、应用级配置 |
| 关于 | 版本号、构建信息、工具状态、检查更新、复制诊断信息 |

导航约束：

- 左侧侧边栏只放主入口，不在页面内部重复做二级导航
- 加固、签名、证书属于主工作区入口，设置、关于属于应用级入口
- 侧边栏收起后只保留图标，页面顺序和位置不变

## 4. 交互原则

- 默认先展示工作流入口，不把设置页当首页
- 文件一旦选中，页面进入“已选择 APK”状态，不继续显示空拖拽态
- 预检失败要明确指出原因，例如“不是 APK”“已加固”“未签名”
- 长路径、错误信息和技术日志允许换行，但按钮文案不换行
- 错误信息优先展示摘要，技术详情支持复制
- 主操作按钮位置稳定，加固进度更新时不应导致页面跳动
- 加固页和签名页切换后保留当前会话，其他页面仍按需挂载
- 侧边栏在任务运行期间显示状态标识，用户可以离开任务页处理其他配置

### 4.1 任务状态与事件

Tauri 后端是任务生命周期的唯一所有者。任务快照包含任务编号、类型、状态、当前步骤、输入输出路径、起止时间、有限条日志和错误信息。前端通过统一的 `task-state` 事件接收不可变快照，并按任务编号过滤，禁止跨任务消费日志。

任务状态固定为：`running`、`succeeded`、`failed`、`cancelled`。加固后自动签名属于同一个加固任务，不能由页面在加固命令返回后临时拼接第二段流程。

进度百分比表示已进入的步骤位置，不承诺等同于精确耗时；界面同时展示真实累计耗时。任务日志保留在内部快照中，不在正常进度界面重复展示；失败时由错误卡提供摘要和复制入口，任何诊断内容都不得包含证书密码等敏感字段。

## 5. 证书与签名配置规则

签名配置不再只有一份，统一由证书管理页维护；设置页不再承载完整签名表单。

| 项目 | 规则 |
|------|------|
| 配置来源 | 证书页统一管理 |
| 应用级配置文件 | `config.toml` |
| 证书数据库 | `shield.db` |
| 签名页 | 不维护临时配置，直接读取证书列表与默认项 |
| 加固后签名 | 加固页显式开关；开启后默认选择默认证书，也可为本次任务选择其他证书，不修改全局默认项 |
| Keystore 类型 | 根据扩展名自动推断，可手动覆盖 |
| Alias | 支持手动输入与 `keytool -list` 自动识别；PKCS12 下 `keytool` 可能规范为小写，校验按大小写不敏感处理并保存实际 Alias |
| 密码 | 允许本地持久化保存，以降低重复输入成本；数据库中以 `enc:v1` 加密格式落盘；创建证书时 Keystore 密码至少 6 位，Key 密码可留空 |
| 保存规则 | 导入证书必须校验通过才能保存；创建证书由 `keytool` 生成后自动入库；编辑证书只更新显示信息与签名偏好 |
| Java 环境 | GUI 启动时统一检测一次 JDK 17+、`keytool`、`javac` 状态，并缓存到全局状态；关于页可手动刷新 |

当前默认签名版本：`V1 + V2 + V3`，`V4` 默认关闭。

证书管理规则：

- 默认证书全局唯一
- 应用内新建证书默认落到应用数据目录 `keystores/`
- 导入已有证书默认仅保存引用路径，不强制复制
- 证书材料保存后视为不可变：`keystore` 文件、类型、Alias、密码不在编辑弹框中修改
- 如需更换证书材料，应重新导入或创建证书，避免把一条记录悄悄改成另一套签名材料
- 创建证书的 Alias 输入允许大写，但保存时以 `keytool -list` 返回的实际 Alias 为准
- 删除 `external` 条目默认只删数据库记录；删除 `managed` 条目时可选择同时删除 keystore 文件
- `keystore_password` 与 `key_password` 允许保存在本地数据库，但必须加密落盘，任何日志与错误输出不得带出密码明文
- 证书列表返回前端时不包含密码明文；签名、自动签名、证书指纹比对只传证书 ID，由 Tauri 后端读取并解密
- 不兼容旧的明文密码记录；遇到旧测试数据时重新导入或创建证书
- 证书页采用单列表管理视图，详情、导入、创建、编辑都通过弹框完成，不在主页面长期展示大块详情区

## 6. 核心工作流

### 6.1 加固

```text
选择 APK
  → 预检文件类型、签名状态、是否已加固
  → 检查 Java / apktool / resources.zip
  → 读取本次“加固后签名”开关与所选证书
  → 调用 shield-core::protect_apk
  → 接收分步进度事件
  → 同一后端任务内可选签名并清理中间产物
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
  → 读取证书列表与默认证书
  → 可切换当前使用证书
  → 必要时做证书指纹对比
  → 调用 shield-core::sign_apk
  → 输出 signed.apk
```

指纹不一致时给出警告，但是否继续由用户决定。

加固页和签名页的输出路径均允许在任务开始前修改。任务开始时固化 APK、证书、签名选项和输出路径；执行中以及完成、失败后的结果展示阶段均不可修改，只有点击“继续加固”或“继续签名”重置任务后才重新开放选择。签名完成后，页面不再展示“开始签名”主按钮，只保留“继续签名”入口，避免完成态出现两个含义接近的操作。

## 7. 后端集成

Tauri command 和 `shield-core` 的关系如下：

```text
protect_apk
  → spawn_blocking
  → shield_core::protect_apk(...)
  → emit("protect-progress")

sign_apk
  → spawn_blocking
  → shield_core::sign_apk(...)

check_apk / check_update / 配置读写
  → main.rs 只做 command 装配
  → 具体实现分别落在 apk_check.rs / updates.rs / app_config.rs
```

Windows 下调用 `java`、`keytool`、`apksigner` 等子进程时，统一复用 `shield_core::utils::no_window_command()`，避免弹出控制台窗口。

## 8. 配置与持久化

GUI 启动时会同时加载应用级配置与证书数据库：

| 平台 | 路径 |
|------|------|
| Linux | `~/.config/dev.mocika.shield-gui/config.toml`、`~/.local/share/dev.mocika.shield-gui/shield.db` |
| macOS | `~/Library/Application Support/dev.mocika.shield-gui/config.toml`、`~/Library/Application Support/dev.mocika.shield-gui/shield.db` |
| Windows | `%APPDATA%\\dev.mocika.shield-gui\\config.toml`、`%APPDATA%\\dev.mocika.shield-gui\\shield.db` |

命名约束：

- `config.toml`：GUI 自动读写的应用级配置
- `shield.db`：证书列表、默认证书、加密后的签名密码、校验状态等结构化数据
- `keystores/`：应用托管的新建 keystore 文件
- CLI 如未来增加人工配置文件，必须与 GUI 自动维护的 `config.toml` 明确区分

## 9. 更新检查

GUI 启动时会调用 GitHub Releases API 检查版本更新，并在关于页提供手动检查入口。关于页同时显示当前检测到的 Java 版本，以及 `keytool` / `javac` 是否可用；如运行期间 Java 环境发生变化，可手动重新检测。关于页还提供“复制诊断信息”，用于 issue 反馈；诊断信息不得包含 APK 路径、证书路径、密码或完整用户目录。

规则如下：

- `patch` / `minor`：顶部提示条
- `major`：弹窗提示
- 忽略状态写入 `dismissed_version`
- 版本比较按 SemVer 处理，支持 `1.2.0-rc.1` 这类预发布版本

## 10. 维护要求

- README、使用指南、发布文档只描述当前这套 GUI
- 新功能优先补齐中英文文案、深浅色主题和窄窗口状态
- 文档里不再保留历史 GUI 的正式入口
- 前端结构保持 `App.tsx -> pages -> components/app -> hooks` 这一层次，不再回到单文件堆叠
- 加固页的事件监听、自动签名收尾、拖拽处理应优先放在独立 hook
- 证书页优先保持桌面单列表管理结构，不回退到网页式长表单堆叠，也不在列表旁常驻详情大面板
- 签名页和关于页同样遵守这一规则：签名流程与数据加载逻辑进 hook，页面文件只保留布局和条件渲染
- 进度侧栏、证书列表项、证书详情弹框、关于信息块这类可复用块优先落在 `components/app/*`

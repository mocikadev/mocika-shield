# 工程架构

本文维护模块职责、工具定位和产物路径。运行时格式见[技术内参](internals.md)，用户操作见[使用指南](../usage.md)。

## 模块与依赖

| 目录 | 职责 | 依赖边界 |
|---|---|---|
| `crates/shield-core/` | APK 预检、加固、DEX 打包、签名、ZIP 对齐与 Java 工具 | 不依赖 GUI、CLI、MCP 或本地数据库 |
| `crates/shield-services/` | 配置、证书、目录、签名身份及业务偏好合并 | 依赖核心，不依赖入口框架，不是后台服务 |
| `apps/shield-cli/` | 参数、结构化输出、受控父子进程与 MCP 适配 | 复用共享服务和核心 |
| `apps/shield-gui/` | Tauri 后端与 React 桌面界面 | 直接链接共享库，不通过 CLI 子进程包装业务 |
| `shield-stub/` | Android Java 壳、Rust Native、工控兼容构建 | 不引入桌面数据库、GUI 或上报逻辑 |
| `skills/mocika-shield/` | 用户 Agent 的独立技能 | 只调用可信本地 CLI，不复制执行实现 |
| `tools/`、`scripts/` | JAR、契约生成、构建、发布和验收脚本 | 生成物不作为用户运行时下载源 |

`apps` 放可执行入口，`crates` 放共享库；不是重复工程。完整目录以源码为准，本文不复制易过时的逐文件树。

## 业务归属

- `shield-core/src/protect_api.rs` 编排加固；Manifest、DEX、资源规则分别归 `protect/`。
- `shield-core/src/signing/` 管理签名过程和普通输出暂存；`java_adapter/` 维护 Java 无损参数协议与工具执行。
- `shield-services/src/certificates/` 拥有证书用例、SQLite、密文和托管文件；`config/` 拥有偏好更新与并发。
- GUI `main.rs` 只启动、注入状态和注册命令；`protect_runner`、`signing`、`cert_service`、`updates` 等负责适配。
- GUI `task_manager` 是任务快照唯一修改者；前端按任务编号消费 `task-state`，不拼接第二段自动签名任务。
- CLI `automation/` 分离计划、材料、进程、协议和提交；MCP 通过当前 CLI 子进程复用，不拥有最终 APK 提交权。

## 工具和资源定位

桌面共包 CLI 识别安装布局后固定使用同包资源，缺失即失败，不借源码、用户目录、PATH 或网络补资源：

| 平台 | CLI | 资源根 |
|---|---|---|
| macOS | `.app/Contents/MacOS/shield` | `Contents/Resources/` |
| Linux deb | `/usr/bin/mocika-shield-cli` | `/usr/lib/MocikaShield/` |
| Linux AppImage | 完整 AppDir 的 `usr/bin/mocika-shield-cli` | 同一 AppDir 的 `usr/lib/MocikaShield/` |
| Windows | 安装目录的 `mocika-shield-cli.exe` | 安装目录 |

各资源根包含 `tools/` 和 `resources/`。具体调用和 AppImage 解包见[接入说明](../ops/agent-setup.md)。

普通未改名的开发／维护 CLI 保留发布包、用户／系统数据目录、开发目录的既有查找规则。显式工具覆盖按调用契约验证，受控任务将其纳入身份绑定，不等于自动回退。

GUI 优先使用 AppImage 的 `APPDIR` 或 Tauri `resource_dir()`，开发构建再定位源码。Windows 传给 Java 的扩展路径统一由 `dunce::simplified()` 规范化；子进程使用 `no_window_command()`，不手写 UNC 替换或散落窗口标志。

## 共享本地资料

GUI、显式共享 CLI 及 MCP 使用同一应用目录：

| 文件 | 内容 |
|---|---|
| `config.toml` | 界面配置、业务偏好和桌面数据通道选择 |
| `shield.db` | 证书、默认项、签名偏好、加密密码和校验状态 |
| `keystores/` | 应用内创建／托管的证书 |

保留应用标识、路径和 `enc:v1` 密文兼容。查询摘要没有密码；实际签名只传证书 ID，由本机服务读取。项目 `shield-cli.toml` 是可选覆盖，不是第二份全局状态。

配置使用锁内重读和字段补丁，SQLite 使用事务及有限忙等待；不持锁运行 Java。活动任务固定材料和有效参数，后续偏好变化不改写任务。详细约束见[共享应用设计](shared-application.md)。

## 构建与产物

| 产物 | 路径 |
|---|---|
| 普通 CLI | `target/release/shield`（Windows 带 `.exe`） |
| 标准／工控资源 | `shield-stub/build/outputs/resources/resources.zip`、`resources-api19.zip` |
| 内部内存候选 | 同目录 `resources-memory.zip`，不进入正式产品 |
| macOS 完整开发应用 | `target/release/bundle/macos/MocikaShield.app` |
| GUI 安装包中间产物 | `target/release/bundle/{appimage,deb,dmg,nsis}/` |
| 发布安装包／校验和 | `dist/<平台>/` |

先构建壳，再构建桌面；运行使用完整资源目录。构建细节见[指南](../ops/build.md)，发布规则见[流程](../process/release.md)，不在本文重复依赖版本和脚本步骤。

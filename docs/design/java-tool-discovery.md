# Java 与 keytool 探测修复

## 背景与目标

Issue #129 在 Windows 上识别到 Java 11，但找不到同一 JDK 内已经存在的 keytool。原逻辑分别从 PATH、JAVA_HOME 查找两个工具，没有关联所选 Java 的安装目录。修复适用于三个桌面平台，不按 Windows 系统版本分支。

## 方案与边界

- 保持 Java 选择顺序：PATH 优先，JAVA_HOME/bin 兜底，不自动切换用户的 Java 版本。
- keytool 优先查找所选 Java 的真实可执行文件同目录（解析符号链接）；如果没有，再向该 Java 查询 `java.home`，处理系统启动器与 Java 8 的 JDK/jre 布局。
- 最后回退到 JAVA_HOME/bin、PATH，保留独立安装工具的使用方式；目录、无执行权限文件不算有效工具。
- Windows 使用原有无控制台子进程辅助函数与路径规范化，不手写 UNC 替换。带空格、中文的路径直接作为进程参数传递。
- 不缓存环境探测结果；关于页面与实际证书、签名、加固操作共用同一探测入口。
- 不新增配置字段、依赖、数据库迁移或 APK 运行时改动。Java 最低要求保持 8；源码构建仍需 JDK 17。
- 查询运行目录只在同目录查找失败时执行，失败则继续兜底；仅查询启动属性，不扫描磁盘、不读取证书或上传路径。

## 架构

工具定位拆到共享核心的私有 `java_tools` 模块，`utils` 保留现有公开接口与版本检查。GUI、CLI 不重复实现探测，不向前端扩大接口。

## 任务与验收

- [x] 增加可复现的回归测试：Java 同目录工具、启动器运行目录、Java 8 jre、符号链接、回退顺序、缺失与非可执行文件、空格与中文路径；先确认新增行为测试失败。
- [x] 实现关联定位与路径规范化，保持现有版本解析、keytool 编码参数、无控制台调用不变。
- [x] 执行核心与 CLI 测试、格式及静态检查；使用本机真实 JDK 验证探测及 keytool 调用。
- [x] 更新环境说明和路线图。修复进入 1.4.x 维护线，不单独推进 Alpha/Beta 功能版本；维护者已授权直接发布 1.4.1。

Windows 原生 GUI 与真实用户环境不能用 macOS 测试代替；交付时明确已验证范围，不宣称所有 Windows 环境均通过。

## 执行记录

- 2026-10-08：确认根因与调用点；开始回归测试及修复。
- 第一组 6 项行为测试在旧探测策略下全部失败，关联定位修复后全部通过；补充缺失回退、属性解析、执行权限及失效链接测试后共 10 项通过。
- `cargo test -p shield-core -p shield-cli`：163 项通过、无失败；既有 5 项 DEX 专项实验按原设置忽略。真实 JDK 测试默认忽略，已另行显式运行。
- macOS 真实 JDK 8（1.8.0_504）、11（11.0.32.1）、17（17.0.20.1）：每个版本分别验证 PATH 仅含 Java 符号链接、仅 JAVA_HOME、PATH 仅含 Java 启动器三个隔离场景，共 9 组通过。每组均实际生成中文 Alias 的临时 JKS，并通过共享入口读取 SHA-256 证书指纹；测试临时证书自动清理，不使用用户证书。
- 测试校正：部分 Java 8 同时提供 JDK/bin 与 JDK/jre/bin 的 keytool，二者均属于同一安装。验收检查工具归属与实际证书读取，不强制所有发行版采用同一目录布局。
- `cargo fmt --all -- --check`、`git diff --check`、`cargo clippy -p shield-core -p shield-cli --all-targets -- -D warnings` 通过。
- 尚未执行 Windows/Linux 原生 GUI 回归；维护者于 2026-10-08 授权同步为 1.4.1，并按 PR、标签、三平台构建、产物检查后公开的流程发布。
- 独立只读审查无严重或重要级阻断项。根据健壮性建议增加跟随链接的普通文件检查，避免 Windows 的失效 `.exe` 链接阻挡有效兜底；同时加强运行目录与环境工具竞争的优先级测试。失效链接回归在 Unix 执行，Windows 原生仍待验收。

Windows 后续人工验收：PATH 仅能找到 Java、JAVA_HOME 与 PATH 指向不同 JDK、Java 启动器、中文及带空格安装目录、扩展 UNC 路径、失效工具链接；确认关于页面与证书读取一致且无额外控制台窗口。Linux 后续验证系统 alternatives 链接与桌面进程继承环境。

真实 JDK 回归入口（只修改测试子进程环境，不改变系统 Java 默认值）：

```bash
SHIELD_TEST_JDK_HOME="<本机 JDK 根目录>" cargo test -p shield-core --test java_toolchain -- --ignored --nocapture
```

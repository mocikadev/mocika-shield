# 中文证书 Alias 编码修复计划

> 执行方式：使用 executing-plans 在当前工程逐项执行，不建立额外工作区。完整阶段验证后再统一提交。

## 目标与约束

修复 keytool 输出与 Rust 解码编码不一致导致中文 Alias 乱码的问题。以此前确认的“明确约定编码、严格校验、完整签名链路回归”为设计依据。

- 保持 Java 8+、JKS/PKCS12、三平台及 Windows 无控制台窗口支持。
- 不改变系统默认 Java、系统编码、用户证书、Alias、密码存储格式或数据库结构。
- 不猜测 GBK/UTF-8；无效 UTF-8 返回可读错误，不把替换字符当成有效 Alias。
- 不自动迁移已有乱码 Alias，修复后重新识别或导入；禁止根据乱码猜测原始值。
- 初始修复阶段不自动发布；维护者后续已授权准备 `1.4.0-beta.4`，合并后由标签触发三平台发布。不将临时私钥、APK 或日志提交到仓库。

## 架构

变更按 L2 审视：GUI 与共享核心同时调用 keytool，需要共同的进程编码约定。新增 `crates/shield-core/src/keytool.rs`，只负责 keytool 命令构建；复用 `no_window_command`，固定语言和 UTF-8 输出，不承载 GUI、存储或密码日志。公开最小接口 `keytool_command(path: impl AsRef<OsStr>) -> Command`。GUI 的 Alias 解析继续留在 signing 模块；核心的证书指纹查询和 GUI 创建证书复用命令入口。无新增第三方依赖。

## 任务与验证

### 真实复现和回归测试

- [x] 在 GUI `cert_unicode_tests.rs` 增加显式忽略的真实 JDK 集成测试，由 `cert_store` 的测试子模块加载，访问现有私有建表逻辑，不扩大生产接口。临时建立英文、中文、混合 Alias 的 JKS/PKCS12 证书，调用真实创建、识别、校验、保存、重新打开数据库接口。
- [x] 使用进程级 `JAVA_TOOL_OPTIONS=-Dfile.encoding=GBK -Dsun.stdout.encoding=GBK -Dsun.stderr.encoding=GBK` 运行该测试，记录修复前失败。该配置仅模拟非 UTF-8 输出，不冒充 Windows 原生测试。
- [x] 在 `signing.rs` 增加无效 UTF-8 数据必须失败的测试，避免乱码进入 Alias 列表。

### 最小修复

- [x] 共享命令显式设置 `-J-Dfile.encoding=UTF-8`、标准输出/错误流编码以及英文语言，覆盖 Java 8/17 与新 JDK。
- [x] 接入 GUI Alias 查询、证书创建、核心指纹查询；数据输出使用严格 UTF-8 解码，诊断错误文本不承担 Alias 数据职责。
- [x] 保持 Windows 路径规范化与隐藏控制台行为，不改变 Java 命令行参数的原始 Unicode 内容。

### 完整回归和交付

- [x] 使用 Java 8、17 分别执行 UTF-8 及 GBK 模拟环境矩阵，增加现有新 JDK 的兼容抽查。
- [x] 通过 `SHIELD_TEST_APK` 指定自有测试 APK，以真实签名接口输出至临时中文路径，核对签名后证书指纹与 keystore 指纹一致；提供工具 JAR 的路径使用 `SHIELD_TEST_APKSIGNER`。
- [x] 执行 `cargo test -p shield-core -p shield-cli -p mocika-shield`、格式检查及完整 macOS `.app` 构建。
- [x] 回填实际结果及 Windows 原生验证限制。测试失败不得记为完成，不能仅凭中文显示正常宣称签名兼容。

## 技术依据

[OpenJDK JEP 400](https://openjdk.org/jeps/400) 区分默认字符集和标准流编码。仅设默认文件编码不足以覆盖所有标准流情况，因此以真实 JDK 子进程字节输出和签名行为验证，而不是仅检查参数文本。

## 2026-09-16 本机执行记录

- 环境：macOS arm64，Corretto 8u504、Temurin 17.0.20.1、Temurin 25.0.4.1。通过单次命令 PATH 选择 JDK，没有改变系统默认配置。
- 修复前：Java 8 + GBK 输出时，英文 Alias 完整签名链路通过；中文 Alias 在创建后的证书校验阶段失败，返回“未在 keystore 中找到指定 alias”。说明影响使用，并非单纯字体问题。
- 修复后：3 个 JDK × UTF-8/GBK × JKS/PKCS12 × 英文/中文/混合 Alias，共 36 组均通过。每组验证实际创建、识别、导入、重新打开 SQLite 后读取、APK 签名、签名验证及指纹一致，原始 keystore 字节不变。混合 Alias 校验返回 keystore 实际小写形式。
- 自有测试 APK：`tests/fixtures/android-smoke-app/app/build/outputs/apk/release/app-release-unsigned.apk`；签名工具：`tools/apksigner.jar`。临时证书、数据库和输出由 `TempDir` 自动清理，不提交真实材料。
- 常规测试：GUI 48 通过、1 项真实 JDK 测试默认忽略；core 141 通过、5 项原有测试忽略；CLI 5 通过。真实 JDK 测试另以 `--ignored` 显式执行。
- `cargo fmt --all -- --check` 与 `git diff --check` 通过；完整 `cargo tauri build --bundles app` 成功，产物为 `target/release/bundle/macos/MocikaShield.app`。未升级版本，仍显示 Beta.3，但这是包含本地修复的构建，不等同于线上 Beta.3。
- 严格 `cargo clippy ... -- -D warnings` 未通过：Rust 1.98 的 `chunks_exact_to_as_chunks` 在现有 `cert_store.rs` 和 `manifest_inspect.rs` 各报告一处，均非本次修改的表达式。本次不扩大范围修订这两处旧代码。
- 本地日志：`/tmp/mocika-chinese-alias-red.log`、`/tmp/mocika-chinese-alias-matrix.log`、`/tmp/mocika-chinese-alias-unit-final.log`、`/tmp/mocika-chinese-alias-app.log`。日志属于本机临时证据，不作为发布资产。

### 尚未验证与发布边界

Beta.4 发布准备复验：版本号同步后，194 项常规测试、20 项脚本测试、前端构建与 lint 均通过；Java 8 + GBK 的 6 组真实证书签名回归再次通过。原 36 组矩阵与本次实现相同；版本变化未引入新的功能逻辑。

- 尚未在 Windows 原生环境验证参数传递、中文代码页和界面操作；macOS 上模拟 GBK 输出不能替代该项。
- 尚未人工操作 `.app` 导入弹窗；已验证的是 GUI 后端真实业务接口和 SQLite 持久化链路。
- 用户真实环境和原始 Alias 未获取，因此不能宣称用户个例已确认修复。
- 维护者已授权将修复作为 `1.4.0-beta.4` 测试版交付：先通过 PR 检查并合并，再推送新标签构建；不覆盖已公开的 Beta.3。Windows 原生验证仍待完成，不将构建成功视为用户场景已验证。

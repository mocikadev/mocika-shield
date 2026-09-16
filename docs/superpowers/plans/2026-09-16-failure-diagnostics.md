# Beta.5 失败诊断实施计划

设计依据：[失败分类与用户确认错误报告](../../design/failure-diagnostics.md)。维护者已确认实施。

## 全局约束

在当前工程和功能分支实施，不创建其他工作树。本地实现阶段不自动提交、推送、部署、启用接口或发布；完成整个阶段再统一交付。维护者随后于 2026-09-16 授权提交 PR 和远端迁移部署，执行结果见文末；客户端发布仍单独推进。全部注释和文档使用中文。匿名统计受原开关控制；报告逐次确认，绝不上传原始错误或日志。APK 壳、证书数据库格式保持不变。

## 固定协议

报告为严格 JSON 对象，所有字段必需，可空字段使用 null：
`report_id`（UUID）、`schema_version=1`、`classifier_version=1`、`sanitizer_version=1`、`app_version`（完整版本）、`build_revision`（null 或 7–40 位小写十六进制）、`build_kind`（release/local/unknown）、`occurred_at`（Unix 秒整数）、`flow`（protect/protect_with_sign/sign/certificate）、`operation`（protect/sign/certificate）、`stage`（prepare/unpack/manifest/dex_runtime/align/execute/sign/unknown）、`code`（设计中的固定错误码）、`platform`（macos/windows/linux/unknown）、`arch`（aarch64/x86_64/x86/arm/unknown）、`java_major`（null 或 8–99）、`java_vendor`（null 或 oracle/openjdk/amazon/adoptium/azul/microsoft/unknown）、`tool_name`（null 或 java/keytool/apktool/apksigner/zipalign）、`tool_version`（null 或最多32字符的数字点版本）、`exit_code`（null 或有符号32位整数）、`evidence`（首期仅允许空数组；信息不足不得透传原文）。

首期无法可靠取得的可选环境字段留空，不能再次执行工具干扰失败任务。证据扩展以后升级 schema。预览展示完整 JSON，摘要为该 JSON 字节 SHA-256；发送完全相同字节，后端禁止接受任意正文。报告最大8192字节。

匿名统计新增可选 `failure_reason_counts` 数组，每项严格为 `flow/operation/stage/code/classifier_version/count`。计数快照采用同键最大值；未提供不删除。分类覆盖标记 `failure_classifier_version=1`，旧客户端缺失为不可用。

### Task 1: 服务端接收与统计协议

范围仅 `tools/stats-worker/**`（其他文件不改）。先写 Node 测试并确认失败，再实现：
- 独立 `failure-reasons.js` 校验固定枚举与维度组合，最多128条原因，count为0–10000整数；保留旧接口兼容。
- 增量迁移 `0003_failure_diagnostics.sql`、schema.sql：原因表、分类覆盖、私有报告表与短期限流桶。
- 独立 `error-reports.js` 严格验证上述协议；POST /reports/errors，默认 `ERROR_REPORTS_ENABLED` 非 true 时503；错误400/413/429/503，无公开读取。
- 同ID同规范载荷200，首次201，冲突409；D1原子事务/触发器保护全局每日1000份和来源每分钟10次。来源使用服务端 `ERROR_REPORT_RATE_SECRET` 按日HMAC的IP桶；缺失密钥或来源拒绝接收，原始IP不保存，不日志正文。报告保留30天，桶24小时，定时清理。
- 原因统计最大累计值且旧客户端不删除原因；趋势增加 `failure_reason_breakdown` 和分类覆盖，不能把缺失历史伪装成UNKNOWN。
- 测试必须覆盖真实SQL（允许 sqlite3 测试桥）与 HTTP：重复/乱序/冲突/旧客户端/字段拒绝/8KiB/关闭/限流/清理/公开接口隔离。
验证：`cd tools/stats-worker && npm test`。不要部署。报告写本计划对应工作空间的 task-1-report.md。

### Task 2: 桌面分类、统计与安全快照

- 核心新增稳定错误码与类型化诊断，在转字符串前提取；不确定为UNKNOWN，不进行全文猜测。
- 独立GUI模块负责冻结版本和结构化上下文；保留原界面错误。
- 同任务终态只一次；自动签名失败保留加固成功；取消不计失败。证书实际操作失败单独归类。
- 新配置字段带默认值、按版本保存原因，旧配置可读；关闭开关不新增原因。
- 快照最多5份/30分钟，报告发送每会话最多5份，同分类仅主动提示一次。前端仅传报告ID+摘要，10秒超时、不自动重试、不打印网络原始错误。
- TDD覆盖分类、隐私样本、版本冻结、旧配置、终态去重、预览发送一致、TTL、重复发送与重试。
验证：`cargo test -p shield-core -p mocika-shield`。

### Task 3: 确认交互与整体验收

- 共用报告弹窗显示完整预览、数据范围、服务地址、30天保存、取消不上传；发送成功显示编号。未开启统计也可主动报告。
- 错误旁保留手动入口；不遮住原错误；配置输入类不自动打扰。
- 更新设置说明、统计运维说明、路线图，Beta.5版本使用现有同步脚本。
- 前端构建、lint、Rust/Worker测试、版本一致性和完整macOS .app构建；不声称未经验证的Windows或线上结果。
- 审查完成后统一交付，报告部署仍保持关闭直到维护者确认。

## 本地验收记录（2026-09-16，持续更新）

- 已同步版本 `1.4.0-beta.5`，未提交、推送、打标签或发布；Beta.4 产物不变。
- Rust 最终全套回归 216 项通过，覆盖固定分类、配置默认兼容、快照隐私、跨端协议、真实 HTTP 字节一致及终态去重；6 项依赖显式环境的测试默认忽略。
- 前端构建与 lint 通过，确认交互测试 4 项通过；维护脚本测试 21 项通过。
- 根据维护者设置页截图，匿名统计卡片改为标题与开关同一行、说明统一内边距并分段；去掉固定标题列与负边距，保留统计保存逻辑和完整隐私边界。前端构建与 lint 复验通过。
- 服务端审查修复后 21 项通过；真实 SQLite 校验迁移、最大累计、幂等、限流、ISO 时间清理、索引及接口隔离。没有连接线上 D1，不把本地通过视为线上验证。
- Java 8 与 Java 17 分别通过真实中文证书导入、保存重开与 APK 签名测试，覆盖 JKS/PKCS12 和英文、中文、混合 Alias；仅使用临时测试证书，未修改全局 Java 环境。
- 桌面首轮审查指出的证书失败返回契约、预检计数、标准 ABI 分类和反馈窗遮挡四项问题已修复并复审通过。
- 最终跨端审查发现的旧版本积压统计可选字段兼容和 apktool 执行边界诊断遗漏已修复并复审通过：旧记录不伪造分类版本；解包/回编在输出转字符串前提取工具名、退出码和固定分类，原错误展示保留。
- 标准壳四 ABI 资源已重新构建成功。Android 4.4 兼容构建首次因 Rust 1.77.2 宿主链接器读取本机新装的 macOS 27 SDK 失败；仅在当前命令指定 Xcode 内的 macOS 26.5 SDK 后，r25c/API19 资源构建及 ELF 审计通过，未修改系统配置或仓库构建策略。
- 轻量发布检查的版本同步、资源、文档引用和敏感文件检查通过；整体检查因工作区尚未提交而返回失败，不能据此宣称已达到发布条件。
- 最终 macOS 应用 `target/release/bundle/macos/MocikaShield.app` 已按最新代码重建，版本为 `1.4.0-beta.5`；按本地发布脚本惯例完成临时签名，严格签名检查及标准/API19 资源 ZIP 完整性检查通过。并非 Apple 公证发行包；人工界面、Windows/Linux 原生及线上验收待进行。
- 首期安全报告不上传自由文本、附件、异常栈或原始日志，`evidence=[]`；Java/工具版本取不到可靠值时保留 null，不声称已经采集。
- 发布说明准备要点：版本关联的固定错误原因；用户逐次确认发送报告；自动签名失败保留加固成功；证书识别/保存和 Android 运行时协议不变。发布时继续沿用固定变更章节，不移动 Beta.4 标签。

## 后续部署门禁

完整本地测试包交付后统一审查提交。线上数据库增量迁移、Worker 部署、报告密钥配置与启用、Beta.5 标签发布需维护者确认；执行顺序见[运维步骤](../../ops/failure-diagnostics.md)。未经执行的验收不得标为完成。

## 提交与线上进展（2026-09-16）

维护者授权后以单条功能提交 `117f2f4` 推送并创建 PR #125。提交前重新运行 Rust 216 项、Worker 21 项、维护脚本 21 项、前端 4 项测试及构建、lint、格式检查，均通过。

远端新增分类字段及三个诊断表，兼容部署后验证关闭入口 503，再配置独立密钥并开启报告。真实接口的统计兼容、最大累计、报告幂等、载荷拒绝、私有读取隔离和每分钟限流通过，测试报告与统计记录已按专用编号清理。详细结果与仍待验收项见[线上执行记录](../../ops/failure-diagnostics.md)。尚未合并 PR、打标签或发布客户端。

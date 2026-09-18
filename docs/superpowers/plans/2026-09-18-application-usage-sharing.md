# 应用加固与签名使用情况分享实施计划

> 开发阶段记录（2026-09-18）：维护者授权按阶段实施，强调保持界面简洁；当时仅开发与本地验证。后续已追加授权按 Beta.6 发布并升级远端，新增执行范围与当前状态以[发布任务](2026-09-18-beta6-release.md)为准，以下开发阶段结果保留原始口径。

## 当前进度

- [x] 确认设计、任务顺序和界面约束：沿用现有样式，只增加单行复选框，不新增卡片或弹窗。
- [x] 核对工作区：在 `docs/application-sharing-plan` 分支，保留已有方案文档，不新建工程副本。
- [x] 基线验证：维护者接受 Xcode 许可协议后重跑，GUI Rust 63 项通过、1 项忽略；Worker 21 项通过；前端构建及 lint 通过。
- [x] 阶段一：身份读取与按包名偏好，26 项针对测试，GUI 后端 89 项通过、1 项忽略，独立复审通过。
- [x] 阶段二：严格服务端协议与维护口径，Worker 38 项通过，独立复审通过；仅本地，远端未迁移或部署。
- [x] 阶段三：GUI 快照、发送和单行交互；GUI 后端 104 通过、1 项既有忽略，前端 11 项通过，独立复审通过；实际视觉检查另列在阶段四。
- [ ] 阶段四：本地回归、完整 `.app` 和整阶段代码审查已完成；人工界面与真实 Tauri 操作待确认（桌面自动化不可用），远端启用另行授权。

**目标：** 加固和签名页分别以单行控件分享成功操作的有限应用信息；新包名默认选中，两页按包名共用退出偏好，分别评估加固与独立签名的跨周期复用。

**架构：** GUI 本地身份解析、偏好及发送服务独立于匿名统计和错误报告；Worker 提供独立私有写入协议及 D1 表。核心加固与 APK 壳保持离线，网络不影响任务结果。

**技术栈：** 现有 Rust/Tauri、React/TypeScript、config.toml、Worker/D1；不新增账号体系或外部解析工具。

**设计依据：** [应用使用情况分享](../../design/application-usage-sharing.md)，执行前完整阅读。

## 全局约束

- 加固页文案仅“分享加固使用情况”，签名页仅“分享签名使用情况”，均不增加第二行、内容预览入口或确认弹窗；说明放设置页及发布说明。
- 新包名默认选中；取消仅对该包名持久有效，重启、升级、同包名换版本不能重置；取消不会影响其他新包名。
- 识别和配置读取不可靠时不发送，不阻止原加固；名称/版本取不到可为 null，不用文件名兜底。
- 两页共用包名偏好，跨页取消立即禁止该包名后续发送并清除未发送记录；已执行任务的界面快照不改，发送前仍检查最新取消状态。
- 控件状态在任务开始固定；取消、独立签名失败及核心加固失败不发送。独立签名成功发送 `sign/sign`；核心加固成功而后续自动签名失败可发送 `protect/protect_with_sign`，自动签名成功也不额外产生独立签名记录。
- 只上传设计列出的十个字段（含 operation、flow、提交编号、协议和说明版本）；不得混入安装 UUID、证书、密码、路径、日志、APK 或硬件身份。
- 8 KiB 载荷；32 条进程内队列、5 分钟 TTL、10 秒超时；仅网络/5xx 在 30 秒后重试一次；不持久化上传队列。
- 服务端明细 180 天、限流桶 24 小时；来源每分钟 20 份、全局每天 2,000 份；默认关闭，无公开读取。
- 当前工程实施，不创建额外 clone，不改 Beta.5 标签；本轮不调整版本号，发布前另行确认。不按小步骤提交，完整阶段统一 PR。

## 文件与接口

| 位置 | 责任 |
|---|---|
| 修改 `apps/shield-gui/src-tauri/src/manifest_inspect.rs` | 扩展 Manifest 本地事实，保持现有预检行为 |
| 新建 `apps/shield-gui/src-tauri/src/application_identity.rs` | 包名/版本/名称解析、资源引用和解析限额 |
| 新建 `apps/shield-gui/src-tauri/src/application_sharing.rs` | 应用偏好、检查引用、不可变任务快照、进程队列和发送 |
| 修改 `app_config.rs`、`main.rs`（同后端目录） | 新配置默认兼容；仅状态注入及 IPC 注册 |
| 修改 `apk_check.rs`、`protect_runner.rs`、`signing.rs`、`task_completion.rs`（同后端目录） | 绑定当前 APK 和两类成功终态，防止自动签名双重捕获；签名不能强制使用加固资格预检 |
| 修改 `apps/shield-gui/src/hooks/use-protect-workflow.ts`、`src/pages/protect-page.tsx`、`src/pages/settings-page.tsx`、`src/lib/tauri.ts`、`src/lib/i18n.ts` | 单行控件、独立设置说明、双语与引用传递 |
| 修改 `apps/shield-gui/src/hooks/use-sign-workflow.ts`、`src/pages/sign-page.tsx` | 签名页单行控件、共享偏好同步与签名任务引用 |
| 新建 `tools/stats-worker/src/application-usage.js` 和同名 `.test.js` | 有限协议、写入、清理及单位测试 |
| 新建 `tools/stats-worker/src/application-usage.integration.test.js` | 真实 SQLite 及 HTTP 合同测试 |
| 新建 `tools/stats-worker/migrations/0004_application_usage.sql`，修改 `schema.sql`、`src/index.js` | 增量迁移、空库建表、独立路由与清理调用；执行前确认迁移序号未占用 |
| 新建 `tests/fixtures/diagnostics/application-usage-v1.json` | Rust/Worker 共用的无敏感信息协议样本 |

任务间使用以下候选接口，正式实现时保持一致，不把前端文本当元数据：

```rust
struct ApplicationIdentity {
    package_name: String,
    app_name: Option<String>,
    app_version_code: Option<String>,
}
struct ApplicationShareChoice {
    inspection_id: String,
    enabled: bool,
}
fn inspect_application(path: &std::path::Path) -> anyhow::Result<ApplicationIdentity>;
fn share_enabled(preferences: &std::collections::BTreeMap<String, bool>, package: &str) -> bool;
```

`inspection_id` 是后端临时随机引用，绑定输入路径与文件检查结果，不上传。前端修改选择提交引用和 enabled；开始任务也仅传引用和 enabled，后端重新核验当前配置与输入。检查引用在切换输入后失效。任务快照内的 Identity 由后端拥有，不将可编辑包名传入网络层。后端依据现有 TaskKind 和自动签名设置派生 operation/flow，不增加前端可任意指定的操作类型参数。

## 阶段一：身份读取与按包名偏好

- [x] 在上述解析/配置模块内先写失败测试：直接 label、资源 label、语言回退、引用循环、超大资源、超大版本码、非法包名、旧配置、新包名默认值、同包名取消跨保存重读及共享偏好、损坏配置不恢复默认选中；实际跨页交互在阶段三验证。
- [x] 运行 `cargo test -p mocika-shield application_`，确认因缺少能力失败，不能用源码字符串断言代替解析行为。
- [x] 扩展 Manifest 身份读取和有界资源解析；偏好以 `application_sharing.preferences` 存储。保持取消记录、不改变匿名开关或证书数据库；取消或重新开启写入失败时均禁止本会话发送。
- [x] 重跑解析/配置测试及原 Manifest 预检测试，确认新增解析失败只影响分享，不将可加固 APK 误判阻塞；独立严格身份路径不改变原预检，新增重复字符串预算和块内边界反例。

偏好测试的最低行为：

```rust
let mut preferences = std::collections::BTreeMap::new();
assert!(share_enabled(&preferences, "com.example.first"));
preferences.insert("com.example.first".to_string(), false);
assert!(!share_enabled(&preferences, "com.example.first"));
assert!(share_enabled(&preferences, "com.example.second"));
```

同包名更换 `app_version_code` 必须继续为 false；测试通过真实 config 保存和重读验证，而非只断言内存 map。

## 阶段二：严格服务端协议与维护口径

- [x] 先写共享样本、额外字段拒绝、8 KiB 流式截断、空值、版本码字符串、非法名称和三种合法操作组合测试，拒绝 `operation=sign, flow=protect_with_sign` 等其他组合；运行 `cd tools/stats-worker && node --test src/application-usage.test.js` 观察失败。
- [x] 实现 `normalizeApplicationUsage(body, byteLength)`，仅返回协议白名单对象和稳定 JSON；`saveApplicationUsage(request, env)` 负责安全状态码；`cleanupApplicationUsage(env)` 负责服务端时间保留窗口。调用边界捕获数据库错误且不输出载荷。
- [x] 写真实 SQLite 测试覆盖首次/重复/冲突、原子限额、回滚、日期清理；创建表、索引、触发器和迁移。仅填充本地测试库，不对线上灌入数据。
- [x] 接入路由，默认关闭；新表缺失不影响旧通道清理及请求。全套 `npm test` 38 项通过，含来源第 20 份、全局第 2,000 份的同编号并发与冲突，以及第 2,001 份 HTTP 拒绝和桶回滚。

共享样本形状（UUID 和字段仅用于测试，不上线发送）：

```json
{
  "submission_id": "4c36288b-8ce1-49fc-9530-7a09172efa0c",
  "schema_version": 1,
  "notice_version": 1,
  "package_name": "com.example.sample",
  "app_name": "演示应用",
  "app_version_code": "4294967296",
  "tool_version": "1.4.0-beta.6",
  "operation": "protect",
  "flow": "protect",
  "success_date": "2026-09-18"
}
```

样本工具版本不代表发布决定；测试的“当前编译版本”及日期应显式替换，避免下次升级或跨日导致假失败。日期验证使用可注入测试时钟。维护查询以 distinct 包名/版本/工具版本/operation/flow/成功日期归并，资源名称改动不增加应用数。测试同包名加固与独立签名分别各一条时，加固应用数=1、签名应用数=1、整体应用并集=1，不能得出整体=2。

## 阶段三：GUI 快照、发送和单行交互

- [x] 先写失败测试：A 包取消后选 B 默认选中、再选 A 仍取消；从加固页切换签名页以及反向切换均沿用同包名选择；快速切换时旧结果不得覆盖新选择；任务启动后只消费冻结引用。使用真实状态转换，不用检索 CSS 字符串代替功能测试。
- [x] `application_sharing` 按检查、协议、队列、发送拆分；注入测试时钟/本地 HTTP 接收器，验证发送前撤销、5 分钟过期、32 条上限、超时、重定向拒绝、固定 UUID 重试及 429 不重试；另覆盖检查引用被实际清理/淘汰后的失败关闭。
- [x] 在对应成功任务终态捕获一次，读取当前输入元数据和冻结选择。检查前后输入身份不一致则不捕获；本地指纹只能用于校验，绝不上传。独立签名成功必须是一条 `sign/sign`，失败/取消是零条；自动签名整条链最多一条 `protect/protect_with_sign`，不产生独立 sign 记录；重复终态不增加请求。
- [x] 两页分别增加单行控件，与任务锁定/重置一致；保存失败显示既有错误提示，不静默恢复勾选。偏好改变通知另一空闲页面重新读取；执行中维持快照但发送前检查最新状态。详细隐私文字只放设置页，实际视觉仍另行验收。
- [x] 运行 `cargo test -p mocika-shield`、`npm run build`、`npm run lint` 及前端行为测试：104 项 Rust 通过、1 项既有忽略，11 项前端通过。
- [ ] 人工检查默认窗口与较窄窗口、深浅色、键盘焦点和任务结束状态；桌面工具无法连接，待完整 `.app` 人工检查。

## 阶段四：完整验收与交付

- [x] 两端共用样本逐字段比较；真实本地 HTTP 收到的载荷只有十个约定字段，不出现安装 UUID、路径、密码、证书、APK、原始错误文本；生产身份入口对两份自建 APK 的环境用例已显式执行通过。
- [x] 使用自建普通 APK 实际验证标准/工控两种资源的加固与独立签名，完成 v1/v2/v3、已加固/已签名及 MSHD 校验；两份自建 APK 完成生产身份入口与本地发送验证。Native 探针因自带规范壳库触发冲突拒绝，未将该次记为加固通过。
- [x] 包名偏好、名称缺失/变化、取消、加固/签名成功与失败、自动签名成功/失败、跨页退出和待发送撤销由 Rust/前端行为测试覆盖；不上传用户业务 APK 元数据。
- [ ] 真实 Tauri 操作的自动签名及跨页完整闭环；现有组合测试不替代真实桌面任务验证。
- [x] 更新 `docs/ops/telemetry.md`、`docs/design/gui.md`、`docs/usage.md`、Release Notes：准确区分三个通道，说明默认参与、按包名退出、私有访问、保留期限和失败不影响加固；明确开发中，不表示已发布 Beta.5 包含此能力。
- [x] 本地 Rust/Worker/前端/维护脚本回归，构建完整 macOS `.app`；资源哈希、ZIP 完整性及本地 ad hoc 签名校验通过。具体结果及默认跳过的环境用例见[回归记录](../../process/application-sharing-regression.md)。
- [ ] 人工确认勾选记忆和界面布局；桌面工具两次启动失败，没有取得实际界面。Windows/Linux 不作未经执行的通过声明。
- [x] 自审与独立整阶段评审数据范围、解析边界、偏好覆盖和并发取消；严格 Manifest 外层校验的最终阻断已修复并复审通过。
- [ ] 人工验收后统一整理提交/PR，不为每个步骤追加 PR；本轮保留工作树，未提交推送。
- [ ] 维护者确认后才增量迁移/部署；先关闭分享入口验证旧客户端，随后用自建样本启用验收，并按专用编号清理测试记录。必要回滚仅关闭新入口，不删除旧表、不移动旧标签。

## 自审结论

每项确认的交互都有对应偏好/快照行为测试，服务端去重与长期应用口径分开，不把包名数据混入匿名通道。主要取舍为未知名称留空、进程关闭不补传、同包名跨项目合并，均已在设计明确。本地实现、自动化回归、真实自建样本与完整应用包验证、独立代码评审已完成；实际界面与 Tauri 操作仍待人工确认，详见[回归记录](../../process/application-sharing-regression.md)。未锁定新版本、未修改运行时、未提交或部署。

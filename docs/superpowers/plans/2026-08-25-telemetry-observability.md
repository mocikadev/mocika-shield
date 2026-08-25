# 1.4.0 匿名统计可诊断性实施计划

> **供执行代理使用：** 必须逐任务使用 `superpowers:subagent-driven-development` 或 `superpowers:executing-plans` 执行；步骤使用复选框追踪。

**目标：** 不增加用户识别或 APK 运行时上报的前提下，按桌面工具版本和固定失败阶段统计匿名聚合数据。

**架构：** 客户端按“日期 + 编译期版本”保存每日计数，失败只记录有限枚举。Worker 使用第二代 D1 表支持三元幂等键，并兼容读取遗留表；趋势接口保留日期总量字段，另返回版本与失败阶段细分。

**技术栈：** Rust、Tauri v2、TOML、Cloudflare Workers、D1、Node.js、Python。

**设计依据：** `docs/superpowers/specs/2026-08-25-1.4-compatibility-observability-design.md`

## 全局约束

- 沿用已有随机 `anonymous_id`，不得新增硬件、账户或最终用户标识。
- 不上传 APK、路径、包名、原始错误文本、证书、密码、签名指纹、设备型号或完整 Android 版本。
- 失败桶仅允许：`protect` 的 `prepare/unpack/manifest/dex_runtime/align/sign`，`sign` 的 `prepare/align/execute`，以及 `task` 的 `cancelled/unknown`。
- 取消不计入失败率；上传失败不得影响主流程。
- 旧客户端和既有 `/stats/trend` 消费者必须保持可用。

---

### 任务 1：客户端版本分桶与失败阶段

**文件：**

- 修改：`apps/shield-gui/src-tauri/src/app_config.rs`
- 修改：`apps/shield-gui/src-tauri/src/telemetry.rs`
- 修改：`apps/shield-gui/src-tauri/src/main.rs`
- 修改：`apps/shield-gui/src-tauri/src/protect_runner.rs`
- 修改：`apps/shield-gui/src-tauri/src/signing.rs`

**接口：** 新增 `TelemetryFailure { operation, stage }`、`record_failure()`、`FailureCountPayload`；`DailyTelemetry` 持久化 `usage_date`、`app_version` 与失败计数表。

- [ ] **步骤 1：先写定向单元测试**

在 `telemetry.rs` 测试模块覆盖：同日 `1.4.0-alpha.1` 和 `1.4.0-alpha.2` 分别累计；`task.cancelled` 不增加 `protect_failed_count`；`ModifyManifest` 映射到 `protect/manifest`；`SignApk` 独立签名映射到 `sign/execute`；未知步骤映射到 `task/unknown`；上传失败不清理本地条目。

- [ ] **步骤 2：运行测试确认新接口不存在**

```bash
cargo test -p shield-gui telemetry::tests::同日不同版本必须分别累计
```

预期：因 `TelemetryFailure` 或版本分桶接口尚未定义而失败。

- [ ] **步骤 3：实现可迁移的本地模型**

内部 map key 使用 `"{usage_date}|{app_version}"`，但 `DailyTelemetry` 显式保存日期和版本。读到旧 map 条目时，从旧 key 取日期、以当前编译期版本补齐空版本，再写回新 key，不丢弃未上传计数。

`record_failure()` 只接受枚举，不接受 `Err(String)`；从结构化任务当前步骤映射失败桶。取消只写 `task/cancelled`。`DailyPayload` 序列化 `failure_counts: [{ operation, stage, count }]`，HTTP 成功后才清理已结束日期对应版本条目。

- [ ] **步骤 4：运行 GUI 后端测试**

```bash
cargo test -p shield-gui telemetry::tests
cargo test -p shield-gui
```

预期：原统计测试、迁移、版本分桶、取消、阶段映射和重试测试全部通过。

- [ ] **步骤 5：提交客户端聚合变更**

```bash
git add apps/shield-gui/src-tauri/src/app_config.rs apps/shield-gui/src-tauri/src/telemetry.rs apps/shield-gui/src-tauri/src/main.rs apps/shield-gui/src-tauri/src/protect_runner.rs apps/shield-gui/src-tauri/src/signing.rs
git commit -m 'feat: 记录匿名统计失败阶段'
```

### 任务 2：Worker 第二代存储与兼容查询

**文件：**

- 创建：`tools/stats-worker/migrations/0002_daily_usage_v2.sql`
- 修改：`tools/stats-worker/schema.sql`、`tools/stats-worker/src/index.js`、`tools/stats-worker/src/index.test.js`

**接口：** `POST /events/daily` 接受可选 `failure_counts`；`GET /stats/trend` 保留 `data`，新增 `schema_version`、`versions`、`failure_breakdown`。

- [ ] **步骤 1：写 Worker 纯函数测试**

导出 `normalizeFailureCounts()` 与 `formatTrendResponse()`。测试拒绝 `protect/raw_error`、接受 `protect/manifest`、兼容缺失 `failure_counts`、合并重复桶，并断言趋势响应仍有旧的 `data` 数组且包含空 `versions`、`failure_breakdown`。

- [ ] **步骤 2：运行测试确认新函数不存在**

```bash
cd tools/stats-worker && npm test
```

预期：新增测试因函数未导出而失败。

- [ ] **步骤 3：新增 D1 迁移**

创建 `daily_usage_v2`，主键为 `(anonymous_id, usage_date, app_version)`；创建 `daily_usage_failure_v2`，主键为 `(anonymous_id, usage_date, app_version, operation, stage)`；为两张表创建以日期、版本开头的查询索引。不得删除、重建或回填遗留 `daily_usage`。

- [ ] **步骤 4：实现幂等写入与联合读取**

Worker 白名单校验失败桶和上限；使用 `DB.batch()` 对同一三元键覆盖 v2 主记录、删除旧桶、再插入规范化桶。日期趋势读取 v2 行，并只加入不存在相同匿名 ID、日期、版本 v2 行的遗留行，避免过渡期重复。版本和失败细分仅返回已保存数据。

响应固定为：

```json
{"schema_version":2,"window_days":14,"data":[],"versions":[],"failure_breakdown":[]}
```

- [ ] **步骤 5：执行本地迁移和接口演练**

```bash
cd tools/stats-worker && npm test
npx wrangler d1 migrations apply mocika-shield-analytics --local
npx wrangler dev --local
```

分别发送旧负载、新 `protect/manifest` 负载和非法阶段负载；预期旧/新为 `204`、非法为 `400`、趋势同时含旧字段与新字段。

- [ ] **步骤 6：提交 Worker 改动**

```bash
git add tools/stats-worker
git commit -m 'feat: 按版本聚合匿名使用统计'
```

### 任务 3：维护统计、设置说明和发布前验证

**文件：**

- 修改：`scripts/project_stats.py`、`scripts/tests/test_project_stats.py`
- 修改：`apps/shield-gui/src/pages/settings-page.tsx`
- 修改：`docs/ops/project-statistics.md`、`docs/process/test-checklist.md`、`.github/release-notes/versions/1.4.0.md`

- [ ] **步骤 1：写脚本兼容性测试**

为 `collect_usage_stats()` 提供旧响应和 `schema_version:2` 响应 fixture。断言旧响应设置 `version_breakdown_available=false`、`failure_breakdown_available=false`，不生成零值；新响应保存 `version_trend`、`failure_breakdown` 和完整日过滤结果。

- [ ] **步骤 2：运行定向测试确认其先失败**

```bash
python3 -m unittest scripts.tests.test_project_stats
```

预期：新断言失败，字段尚未产生。

- [ ] **步骤 3：实现维护数据和用户说明**

脚本保留原总量，细分字段不存在时明确不可用。设置页说明为：只发送桌面工具的匿名每日启动、加固、签名和固定失败阶段计数；不上传 APK、路径、包名、证书、密码或错误日志。不得称为应用活跃、用户追踪或安装量。

- [ ] **步骤 4：执行全量验证**

```bash
python3 -m unittest scripts.tests.test_project_stats
cd tools/stats-worker && npm test
cargo test -p shield-gui
make test
git diff --check
```

预期：全部通过。

- [ ] **步骤 5：预演部署并提交收口文档**

先在本地 D1 与 Workers 预演环境验证迁移；公开响应不得包含 `anonymous_id`、平台、架构或原始错误。经维护者授权后再执行生产迁移和部署。

```bash
git add scripts apps/shield-gui/src/pages/settings-page.tsx docs .github/release-notes
git commit -m 'docs: 说明匿名统计数据边界'
```

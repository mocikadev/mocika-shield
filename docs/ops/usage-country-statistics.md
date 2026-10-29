# 匿名使用国家／地区统计运维

## 范围与控制

本能力只附属于桌面工具现有匿名统计上报，沿用设置中的开关。不改客户端协议或 UI，不需要升级桌面版本，不在加固后的 APK 中添加联网行为。

Worker 从 Cloudflare 的 `request.cf.country` 取得国家／地区代码，仅保存随机匿名安装标识、观察 UTC 日期、两位代码和首次观察时间。不保存原始 IP、省市、经纬度、任意请求头或正文，不关联应用包名或错误报告。`XX`、`T1`、缺失和非法代码均不保存，查询时归入未知。

`USAGE_COUNTRY_ENABLED` 只有字符串 `true` 才启用采集；其他值或缺失均停用。该变量只供维护者控制，不增加用户设置项。部署配置默认开启，保留清理不受开关影响。

当日第一次成功记录有效地区后固定，同日换版本、换网络或重试都不覆盖。客户端补传过去 30 天的快照时，不把当前出口写成历史地区；仅接受使用日期与服务端接收 UTC 日期一致的观察。地区附属写入失败只记录固定提示，原计数已保存仍返回 204。

## 数据库与保留

`daily_usage_country` 主键为 `(anonymous_id, observed_date)`；`idx_daily_usage_country_date` 支持日期筛选和清理。该表独立于按版本累计的原任务计数。未知地区不写空行，之后同日的有效请求仍可补齐观察。

保留含当天的 90 个 UTC 日期。既有每日 `17 3 * * *` 定时任务删除 `observed_date < date('now', '-89 days')` 的地区行；任务执行前可能短暂多保留一个调度周期。不清理或改写原计数、应用分享或报告，其他数据保留策略仍按原文档执行。

## 部署顺序

1. 记录当前 Worker 部署版本、变量名称、D1 绑定和结构，确认操作目标是 `mocika-shield-stats-api` 与 `mocika-shield-analytics`。
2. 执行 **仅新增表与索引** 的 `tools/stats-worker/migrations/0005_usage_country.sql`。不得向线上重放整个 `schema.sql` 或旧迁移。
3. 检查新增表结构和索引，确认原表定义未改变；新表起初为空，不回填历史。
4. 从阶段已验证代码部署，保留已有变量、密钥、绑定、域名、兼容日期及定时任务。
5. 验证原趋势、汇总正常，地区路径仍返回 404；直接 D1 查询新增表。生产不构造普通用户测试事件，暂时没有观察时记录“待自然上报”。

```bash
cd tools/stats-worker
npx --yes wrangler@4.134.0 d1 execute mocika-shield-analytics --remote --file migrations/0005_usage_country.sql
npx --yes wrangler@4.134.0 deploy --keep-vars
```

现有授权也可通过 D1 API 执行同一份增量 SQL。令牌从已配置环境或工具中读取，不粘贴进命令、文档或日志。执行前确认数据库 ID 与 `wrangler.toml` 一致。

## 私有查询

使用 [`country-distribution.sql`](../../tools/stats-worker/queries/country-distribution.sql)，经既有授权 D1 API 参数化执行。三个位置参数依次是：

| 参数 | 内容 |
|---|---|
| 起始 UTC 日期 | `YYYY-MM-DD`，如近 7 日取今天减 6 日 |
| 结束 UTC 日期 | `YYYY-MM-DD`，一般为今天；SQL 不纳入未来日期 |
| 排除标识 | 已登记本机匿名标识；不排除时传 `null`，不得将真实标识写入仓库 |

单日查询令起止日期相同。窗口不超过 90 个 UTC 日期；近 7／30 日均包含未结束的当天。活跃分母为旧新计数表的实例并集，跨版本、跨日期去重。每个实例只取窗口内最近一个有效观察日；不借用窗口外的地区。无有效观察的活跃实例仍进入分母，`country_code = null` 展示为“未知”。

结果字段：

| 字段 | 含义 |
|---|---|
| `country_code` | 国家／地区代码，`null` 为未知 |
| `active_instances` | 该组匿名安装实例数 |
| `total_active_instances` | 整个窗口去重活跃分母，每行相同 |
| `located_instances` | 有有效地区的实例数，每行相同 |
| `coverage_percent` | 有效地区覆盖率，百分数，每行相同 |

各组人数相加必须等于分母，但不能把每行重复的总数再次相加。没有活跃时返回空数组，表示该窗口无活跃，覆盖率不适用；查询失败则是不可用，不是零。

地区明细与聚合均不添加到 `/stats/trend`、`/stats/summary`、`stats` 分支历史或公开页面。实际查看仍由维护者直接查询 D1，不另建公开鉴权入口。

## 解释限制

- 地区是请求的网络出口，不是用户的实际位置；VPN、代理或企业网络会影响结果。
- 随机安装实例不等于自然人、企业、设备硬件或应用数，不能据此识别具体使用者。
- 没有历史回填；上线初期 7／30 日的未知比例较高，需要同时报告覆盖率。
- 未开启统计或上报失败的客户端不在分母内，也不能据此估算全部国内外用户。

## 回滚

优先关闭采集，保留清理和原计数；例如在当前已验证源码下部署覆盖变量：

```bash
npx --yes wrangler@4.134.0 deploy --keep-vars --var USAGE_COUNTRY_ENABLED:false
```

恢复时部署仓库默认配置。必要时按已记录版本回滚旧 Worker，新增表保留，不删表、不回填或修改原数据。旧 Worker 没有新增清理逻辑，若持续回滚，需要另外安排同一条地区保留清理 SQL。

设计与任务记录见[设计](../superpowers/specs/2026-10-29-usage-country-statistics-design.md)及[实施计划](../superpowers/plans/2026-10-29-usage-country-statistics.md)。

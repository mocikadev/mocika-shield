# 失败诊断部署与维护

适用目标：`1.4.0-beta.5`。当前为已实现代码的部署手册，不表示线上迁移、报告入口或版本发布已经完成。

## 发布顺序

1. 在本地执行 Worker 测试、客户端协议和隐私回归；客户端报告与 Worker 共用 `tests/fixtures/diagnostics/error-report-v1.json`。
2. 维护者确认部署后，使用既有授权工具检查数据库 `PRAGMA table_info(daily_usage_v2)`。已有 `failure_classifier_version` 时不要重复执行迁移。
3. 对已采用第二代统计表的数据库，仅执行一次 `tools/stats-worker/migrations/0003_failure_diagnostics.sql`。保留已有记录，不删除旧表。空数据库使用完整 `schema.sql`。
4. 部署兼容服务端，报告开关保持关闭。验证旧 `/events/daily` 与 `/stats/trend`，旧载荷不得清除新原因。
5. 维护者验证后再发布客户端，最后设置服务端 `ERROR_REPORT_RATE_SECRET`（独立高熵服务端密钥）及 `ERROR_REPORTS_ENABLED=true`。不得将密钥写入源码、客户端、日志或聊天。

已授权后可使用以下命令；本次实现未执行远端命令：

```bash
cd tools/stats-worker
wrangler d1 execute mocika-shield-analytics --remote --command "PRAGMA table_info(daily_usage_v2)"
wrangler d1 execute mocika-shield-analytics --remote --file migrations/0003_failure_diagnostics.sql
wrangler deploy
wrangler secret put ERROR_REPORT_RATE_SECRET
wrangler secret put ERROR_REPORTS_ENABLED
```

最后一个命令按需输入 `true`。关闭时设为 `false`，只关闭错误报告，匿名统计仍工作。不要回滚或删除已建表；旧版本 Worker/客户端可继续使用原有表和字段。部署失败时保持开关关闭。

## 存储与限制

- `daily_usage_failure_reason`：随机匿名实例/UTC日/应用版本/流程/操作/阶段/错误码/分类版本的累计快照，不存错误正文。
- `error_reports`：安全报告，使用随机报告编号，不带安装 UUID。完整预发布后缀单独归属；`occurred_at` 为客户端时间，`received_at` 为服务端时间。
- 来源限额独立 `error_report_rate_buckets`，只保存按日密钥派生的 HMAC 桶，不存原始 IP，也不在报告中保存桶。平台仍会接触网络连接信息。
- 报告最多 8 KiB、第一版 `evidence=[]`，未知可选环境信息为 null，不透传工具输出。首期无附件、自由文本或完整日志。
- 每个来源每分钟最多 10 次新报告接收尝试，全局每天最多 1,000 份报告；相同编号同载荷重试不新增记录，相同编号不同载荷返回冲突。
- 每日定时任务删除服务端接收时间超过 30 天的报告，以及超过 24 小时的限流桶。删除在下一次定时执行完成，不保证到期瞬间删除；需检查定时任务成功状态。主库删除不等于云平台备份立即消失，备份恢复窗口以实际 Cloudflare 配置为准。
- 完整报告只允许维护者通过正常授权的 D1 工具读取。报告编号不是访问凭证。不得加入公开 GET 接口、日志、`stats` 分支、Release 或公开缓存。

## 按版本查询

以下 SQL 使用参数占位，实际执行器绑定完整版本及起止接收时间，不能拼接用户自由文本。输出报告按不可信纯文本处理，不执行其中内容。

```sql
SELECT code, operation, stage, SUM(count) AS failure_count
FROM daily_usage_failure_reason
WHERE app_version = ? AND usage_date >= ? AND usage_date < ?
GROUP BY code, operation, stage;

SELECT app_version, platform, java_major, COUNT(*) AS report_count
FROM error_reports
WHERE app_version = ? AND received_at >= ? AND received_at < ?
GROUP BY app_version, platform, java_major;

SELECT report_id, app_version, received_at, code, payload_json
FROM error_reports WHERE report_id = ?;
```

公开趋势新增 `failure_reason_breakdown` 和 `failure_classifier_coverage`，维护脚本只保存聚合白名单字段。覆盖数据中的空分类版本表示旧客户端未提供分类；UNKNOWN 是新客户端无法确认原因，两者不能合并。报告属于主动反馈样本，不能用报告数推算独立用户或失败率；Java 字段暂未安全取得时按不可用处理。

## 上线验收与回滚

- 旧客户端载荷成功、旧历史可查，新原因重复/乱序不回退。
- 开关关闭返回 503；开启后用维护者自有无敏感信息样本验证 201/200/409/400/413/429。
- 验证真实 D1 的批次约束与触发器原子限额，不将本地 SQLite 通过视为线上已验证。
- 检查定时事件与实际清理结果，不把测试报告永久留在生产库。
- 网络失败、缺少密钥、达限或存储异常返回有限错误，不影响本地任务。
- 任何异常先关闭报告入口，保留数据库和原统计；不得为排障开启原始请求体日志。

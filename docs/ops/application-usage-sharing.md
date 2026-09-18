# 应用使用情况分享运维

> 2026-09-18：经维护者授权，远端增量迁移、Worker 部署及新入口启用已完成，线上自建样本验收通过。客户端目标为 `1.4.0-beta.6`，发布进度见[任务清单](../superpowers/plans/2026-09-18-beta6-release.md)。现有 Beta.5 不包含此能力。

设计与字段边界见[应用使用情况分享](../design/application-usage-sharing.md)。这不是匿名使用统计，也不是错误报告。

## 上线顺序

维护者确认发布安排后才执行以下步骤：

1. 记录目标 Worker、D1 数据库、现有迁移和部署版本；按当前运维流程建立可恢复备份。
2. 对已有数据库仅执行 `migrations/0004_application_usage.sql` 增量迁移，不以 `schema.sql` 替代升级。
3. 配置独立高熵密钥 `APPLICATION_SHARE_RATE_SECRET`，仅保存为 Worker secret，不写代码、命令参数、文档或客户端。
4. 保持 `APPLICATION_SHARING_ENABLED` 关闭部署，验证匿名统计和错误报告旧路由正常、新路由返回 503、没有公开读取。
5. 确认版本说明与设置页已披露默认参与、按包名退出及发送范围后，再启用 `APPLICATION_SHARING_ENABLED="true"`。
6. 仅用自建样本验证首次提交、相同编号重试、冲突、取消及版本关联；记录专用测试提交编号，按编号清理，不能按日期删除当天真实用户数据。
7. 检查每日清理调用、真实 SQL 回归及线上触发配置，再发布客户端；部署后的首轮定时执行另作观察，不冒称已发生。任一阶段异常，可关闭新入口回滚接收，不删除历史表、不移动已发布标签。

不能拿用户私下提供的业务 APK 做开发上报测试。不要在 CI 输出请求正文、数据库明细、包名、名称或来源 IP。

## 查询口径

仅维护者经已授权 D1 接口查询，不增加公开 GET 接口、管理站、stats 分支快照或 Release 附件。成功记录只说明工具报告该操作成功，不证明应用上线、正式发布或所有权。

以下查询针对已接收且仍在保留期内的数据；整体应用数是包名并集，不是加固应用数与签名应用数之和：

```sql
SELECT
  COUNT(DISTINCT package_name) AS all_apps,
  COUNT(DISTINCT CASE WHEN operation = 'protect' THEN package_name END) AS protected_apps,
  COUNT(DISTINCT CASE WHEN operation = 'sign' THEN package_name END) AS signed_apps
FROM application_usage_submissions;

SELECT COUNT(*) AS apps_with_both_operations
FROM (
  SELECT package_name
  FROM application_usage_submissions
  GROUP BY package_name
  HAVING COUNT(DISTINCT operation) = 2
);
```

跨版本和成功日期归并，名称变化不增加应用数；不能以提交行数推算用户人数：

```sql
SELECT DISTINCT package_name, app_version_code, tool_version,
       operation, flow, success_date
FROM application_usage_submissions
ORDER BY success_date DESC, package_name;
```

`protect/protect_with_sign` 只证明核心加固成功，包括后续自动签名失败的情形。仅凭该流程字段不能推断签名成功。独立签名与加固的复用应分别计算，低频使用应按发布周期观察，而不是只看日活。

## 保留与撤回

- 明细按服务端 `received_at` 保留 180 天，来源限流桶保留 24 小时，由每日清理删除过期记录；不永久保留带包名的汇总。
- 日清理不是精确到秒的删除承诺，主库删除也不代表备份立即消失。
- 客户端取消分享只停止同包名后续加固及签名分享，不自动删除已经收到的记录；已发出的请求可能已被接收。
- 用户提出删除请求时由维护者私下核实范围后处理，不建立凭公开包名即可匿名删除的接口。公开展示应用名称或案例另行征得授权。

## 配额与故障

来源每分钟 20 份、全局每日 2,000 份，接收载荷最多 8 KiB。来源桶由服务端 IP、日期与通道前缀的 HMAC 产生，不存储原始 IP，不关联匿名设备或错误报告。

客户端待发送记录仅存进程内，最多 32 条、有效期 5 分钟；关闭后不补传。10 秒超时、不跟随重定向；网络错误或 5xx 最多 30 秒后重试一次，429 与其他 4xx 不自动重试。接收故障不得影响本地加固和签名结果。

## 2026-09-18 上线执行记录

- 使用 Wrangler `4.134.0`，数据库 `mocika-shield-analytics`，Worker `mocika-shield-stats-api`。
- 迁移前完整导出 SQL 备份（106,800 字节），仅保留在本机私有运维目录，目录 0700、文件 0600；不提交、上传或附到 Release。
- 发现 `0003_failure_diagnostics.sql` 的实际字段、表、索引与触发器均已存在，但迁移账本仅到 0002。核实与历史直接执行记录一致后，仅补登记 0003；没有重放旧 ALTER。
- 随后仅应用 `0004_application_usage.sql`：新增两张应用分享表、三个业务索引及全局每日限额触发器。迁移账本现为 0001–0004，旧表保留。
- 创建独立限流 secret，关闭新入口部署；保留原错误报告 secret、DB 绑定与每日 `17 3 * * *` 清理配置。部署使用 `--keep-vars`，密钥未写入源码或输出。
- 关闭状态验证新入口 503、旧趋势 200、旧匿名载荷 204、非法错误报告 400；随后启用分享入口。
- 自建冒烟包验证新建 201、重复 200、冲突 409、非法字段 400、超大正文 413、来源分钟上限 429、公开读取 404；D1 核对 `1.4.0-beta.6` 的加固和独立签名两类操作关联正确。
- 本轮样本及旧匿名测试行已按专用随机编号精确删除；不按版本、日期或包名批量删数据。共享来源桶保留至正常过期，不混淆为真实应用使用量。
- 新代码部署版本为 `b4fb091a-08a1-4288-8a5f-7e424de1812f`；启用 secret 后的生效版本为 `48165a9b-ba95-4801-b481-1c7cbbafb98a`（2026-09-18 UTC 06:37），旧版本 `aff95d53-5cc7-4907-8be3-47518428836b` 留作定位。
- 180 天明细与 24 小时桶的删除边界通过本地真实 SQL 回归；部署后的首轮定时执行尚待自然触发，不进行线上过期数据造假或全局限额灌满测试。

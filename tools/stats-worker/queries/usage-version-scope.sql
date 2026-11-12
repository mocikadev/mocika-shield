-- 私有只读维护查询：起始 UTC 日、结束 UTC 日、成功取得的公开版本 JSON 数组、排除的自测标识或 NULL。
-- 版本匹配不认证客户端来源；不同分组的实例可能重叠，不能相加。
WITH period AS (
  SELECT date(?) AS start_date, min(date(?), date('now')) AS end_date,
    ? AS published_versions, ? AS excluded_id
), unified AS (
  SELECT anonymous_id, usage_date, app_version, app_start_count,
    protect_success_count, protect_failed_count, sign_success_count, sign_failed_count
  FROM daily_usage_v2, period
  WHERE usage_date BETWEEN start_date AND end_date
    AND (excluded_id IS NULL OR anonymous_id <> excluded_id)
  UNION ALL
  SELECT old.anonymous_id, old.usage_date, old.app_version, old.app_start_count,
    old.protect_success_count, old.protect_failed_count, old.sign_success_count, old.sign_failed_count
  FROM daily_usage old, period
  WHERE old.usage_date BETWEEN start_date AND end_date
    AND (excluded_id IS NULL OR old.anonymous_id <> excluded_id)
    AND NOT EXISTS (
      SELECT 1 FROM daily_usage_v2 current WHERE current.anonymous_id = old.anonymous_id
        AND current.usage_date = old.usage_date AND current.app_version = old.app_version
    )
), classified AS (
  SELECT unified.*, CASE WHEN EXISTS (
    SELECT 1 FROM period, json_each(period.published_versions) versions
    WHERE versions.value = unified.app_version
  ) THEN 'release_version_match' ELSE 'unmatched_version' END AS version_scope
  FROM unified
), scoped AS (
  SELECT 'all_received' AS scope, classified.* FROM classified
  UNION ALL
  SELECT version_scope AS scope, classified.* FROM classified
)
SELECT scope, COUNT(DISTINCT anonymous_id) AS active_instances,
  SUM(app_start_count) AS app_starts, SUM(protect_success_count) AS protect_successes,
  SUM(protect_failed_count) AS protect_failures, SUM(sign_success_count) AS sign_successes,
  SUM(sign_failed_count) AS sign_failures
FROM scoped GROUP BY scope ORDER BY scope;

-- 私有只读：起止 UTC 日、排除自测标识或 NULL；仅返回声明身份的聚合，不输出匿名标识。
WITH period AS (
  SELECT date(?) AS start_date,min(date(?),date('now')) AS end_date,? AS excluded_id
)
SELECT product_name,application_id,app_version,build_revision,
  COUNT(DISTINCT anonymous_id) AS active_instances,
  MIN(usage_date) AS first_usage,MAX(usage_date) AS last_usage,
  SUM(app_start_count) AS app_starts,SUM(protect_success_count) AS protect_successes,
  SUM(protect_failed_count) AS protect_failures,SUM(sign_success_count) AS sign_successes,
  SUM(sign_failed_count) AS sign_failures
FROM daily_usage_v3,period
WHERE usage_date BETWEEN start_date AND end_date AND (excluded_id IS NULL OR anonymous_id<>excluded_id)
GROUP BY product_name,application_id,app_version,build_revision
ORDER BY active_instances DESC,product_name,application_id,app_version,build_revision;

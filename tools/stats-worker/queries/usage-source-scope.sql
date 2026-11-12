-- 私有只读：起止 UTC 日、公开版本 JSON 数组、本项目名称、应用标识、排除自测标识或 NULL。
-- 身份匹配不认证来源；各组实例可能重叠，不可相加。旧记录始终未知，不按版本补身份。
WITH period AS (
  SELECT date(?) AS start_date,min(date(?),date('now')) AS end_date,
    ? AS versions,? AS product_name,? AS application_id,? AS excluded_id
), unified AS (
  SELECT anonymous_id,usage_date,app_version,daily_usage_v3.product_name,daily_usage_v3.application_id,build_revision,
    app_start_count,protect_success_count,protect_failed_count,sign_success_count,sign_failed_count
  FROM daily_usage_v3,period
  WHERE usage_date BETWEEN start_date AND end_date AND (excluded_id IS NULL OR anonymous_id<>excluded_id)
  UNION ALL
  SELECT anonymous_id,usage_date,app_version,NULL,NULL,NULL,
    app_start_count,protect_success_count,protect_failed_count,sign_success_count,sign_failed_count
  FROM daily_usage_v2,period
  WHERE usage_date BETWEEN start_date AND end_date AND (excluded_id IS NULL OR anonymous_id<>excluded_id)
  UNION ALL
  SELECT old.anonymous_id,old.usage_date,old.app_version,NULL,NULL,NULL,
    old.app_start_count,old.protect_success_count,old.protect_failed_count,old.sign_success_count,old.sign_failed_count
  FROM daily_usage old,period
  WHERE old.usage_date BETWEEN start_date AND end_date AND (excluded_id IS NULL OR old.anonymous_id<>excluded_id)
    AND NOT EXISTS(SELECT 1 FROM daily_usage_v2 current WHERE current.anonymous_id=old.anonymous_id
      AND current.usage_date=old.usage_date AND current.app_version=old.app_version)
), classified AS (
  SELECT unified.*,CASE
    WHEN unified.product_name IS NULL THEN 'unknown_source'
    WHEN unified.product_name=period.product_name AND unified.application_id=period.application_id
      AND EXISTS(SELECT 1 FROM json_each(period.versions) WHERE value=unified.app_version)
      THEN 'project_identity_match'
    ELSE 'other_declared_identity' END AS scope
  FROM unified,period
), scoped AS (
  SELECT 'all_received' AS scope,unified.* FROM unified
  UNION ALL SELECT scope,anonymous_id,usage_date,app_version,product_name,application_id,build_revision,
    app_start_count,protect_success_count,protect_failed_count,sign_success_count,sign_failed_count FROM classified
)
SELECT scope,COUNT(DISTINCT anonymous_id) AS active_instances,
  COUNT(DISTINCT CASE WHEN product_name IS NOT NULL THEN anonymous_id END) AS identity_instances,
  ROUND(100.0*COUNT(DISTINCT CASE WHEN product_name IS NOT NULL THEN anonymous_id END)
    /NULLIF(COUNT(DISTINCT anonymous_id),0),1) AS identity_coverage_percent,
  SUM(app_start_count) AS app_starts,SUM(protect_success_count) AS protect_successes,
  SUM(protect_failed_count) AS protect_failures,SUM(sign_success_count) AS sign_successes,
  SUM(sign_failed_count) AS sign_failures
FROM scoped GROUP BY scope ORDER BY scope;

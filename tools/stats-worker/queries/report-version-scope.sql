-- 私有只读查询：起始 UTC 日、结束 UTC 日、成功获取的公开版本 JSON 数组。
-- 分享按成功日期、错误报告按服务端接收日期；不输出包名、正文或安装标识。
WITH period AS (
  SELECT date(?) AS start_date, min(date(?), date('now')) AS end_date,
    ? AS published_versions
), reports AS (
  SELECT 'application_share' AS channel, tool_version AS version, package_name,
    operation = 'protect' AS protected
  FROM application_usage_submissions, period
  WHERE success_date BETWEEN start_date AND end_date
  UNION ALL
  SELECT 'error_report', app_version, NULL, 0
  FROM error_reports, period
  WHERE date(received_at) BETWEEN start_date AND end_date
), classified AS (
  SELECT reports.*, CASE WHEN EXISTS (
    SELECT 1 FROM period, json_each(period.published_versions) versions
    WHERE versions.value = reports.version
  ) THEN 'release_version_match' ELSE 'unmatched_version' END AS version_scope
  FROM reports
), scoped AS (
  SELECT 'all_received' AS scope, classified.* FROM classified
  UNION ALL
  SELECT version_scope AS scope, classified.* FROM classified
)
SELECT channel, scope, COUNT(*) AS records,
  CASE WHEN channel = 'application_share' THEN COUNT(DISTINCT package_name) END AS packages,
  CASE WHEN channel = 'application_share' THEN COUNT(DISTINCT CASE WHEN protected THEN package_name END) END AS protected_packages
FROM scoped GROUP BY channel, scope ORDER BY channel, scope;

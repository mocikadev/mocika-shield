-- 仅维护者经授权查询 D1；三个位置参数依次为起始 UTC 日期、结束 UTC 日期、排除的自测标识或 NULL。
-- 单日令起止日期相同；窗口建议不超过 90 日，不把未来日期纳入统计。
WITH period AS (
  SELECT date(?) AS start_date, min(date(?), date('now')) AS end_date, ? AS excluded_id
), active AS (
  SELECT anonymous_id FROM daily_usage_v2, period
  WHERE usage_date BETWEEN start_date AND end_date
    AND (excluded_id IS NULL OR anonymous_id <> excluded_id)
  UNION
  SELECT anonymous_id FROM daily_usage, period
  WHERE usage_date BETWEEN start_date AND end_date
    AND (excluded_id IS NULL OR anonymous_id <> excluded_id)
), observations AS (
  SELECT anonymous_id, country_code,
    ROW_NUMBER() OVER (PARTITION BY anonymous_id ORDER BY observed_date DESC) AS position
  FROM daily_usage_country, period
  WHERE observed_date BETWEEN start_date AND end_date
), assigned AS (
  SELECT active.anonymous_id, observations.country_code
  FROM active LEFT JOIN observations
    ON active.anonymous_id = observations.anonymous_id AND observations.position = 1
), totals AS (
  SELECT COUNT(*) AS total_active_instances, COUNT(country_code) AS located_instances FROM assigned
)
SELECT country_code, COUNT(*) AS active_instances, total_active_instances, located_instances,
  ROUND(100.0 * located_instances / NULLIF(total_active_instances, 0), 1) AS coverage_percent
FROM assigned CROSS JOIN totals
GROUP BY country_code, total_active_instances, located_instances
ORDER BY active_instances DESC, country_code ASC;

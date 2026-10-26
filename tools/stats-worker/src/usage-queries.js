// 只读统计共享同一新旧表合并规则，不修改上报或存储。
const UNIFIED_USAGE = `
  WITH unified_usage AS (
    SELECT anonymous_id, usage_date, app_version, app_start_count, protect_start_count,
      protect_success_count, protect_failed_count, sign_success_count, sign_failed_count
    FROM daily_usage_v2
    WHERE usage_date BETWEEN date('now', ?) AND date('now')
    UNION ALL
    SELECT legacy.anonymous_id, legacy.usage_date, legacy.app_version, legacy.app_start_count,
      legacy.protect_start_count, legacy.protect_success_count, legacy.protect_failed_count,
      legacy.sign_success_count, legacy.sign_failed_count
    FROM daily_usage legacy
    WHERE legacy.usage_date BETWEEN date('now', ?) AND date('now')
      AND NOT EXISTS (
        SELECT 1 FROM daily_usage_v2 current
        WHERE current.anonymous_id = legacy.anonymous_id
          AND current.usage_date = legacy.usage_date
          AND current.app_version = legacy.app_version
      )
  )
`;

export async function queryUsage(env, days, byVersion = false) {
  const result = await env.DB.prepare(`${UNIFIED_USAGE}
    SELECT usage_date, ${byVersion ? "app_version," : ""}
      COUNT(DISTINCT anonymous_id) AS active_devices,
      SUM(app_start_count) AS app_starts,
      ${byVersion ? "SUM(protect_start_count) AS protect_starts," : ""}
      SUM(protect_success_count) AS protect_successes,
      SUM(protect_failed_count) AS protect_failures,
      SUM(sign_success_count) AS sign_successes
      ${byVersion ? ", SUM(sign_failed_count) AS sign_failures" : ""}
    FROM unified_usage
    GROUP BY usage_date ${byVersion ? ", app_version" : ""}
    ORDER BY usage_date ASC ${byVersion ? ", app_version ASC" : ""}
  `).bind(`-${days - 1} days`, `-${days - 1} days`).all();
  return result.results || [];
}

export async function queryActivityWindows(env) {
  const result = await env.DB.prepare(`${UNIFIED_USAGE}
    SELECT 7 AS window_days, date('now', '-6 days') AS start_date,
      date('now') AS end_date, COUNT(DISTINCT anonymous_id) AS active_devices
    FROM unified_usage WHERE usage_date >= date('now', '-6 days')
    UNION ALL
    SELECT 30 AS window_days, date('now', '-29 days') AS start_date,
      date('now') AS end_date, COUNT(DISTINCT anonymous_id) AS active_devices
    FROM unified_usage
    ORDER BY window_days
  `).bind("-29 days", "-29 days").all();
  return result.results || [];
}

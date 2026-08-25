import { getCurrentSummary } from "./github-summary.js";

const ALLOWED_EVENTS = new Set([
  "app_start_count",
  "protect_start_count",
  "protect_success_count",
  "protect_failed_count",
  "sign_success_count",
  "sign_failed_count",
]);

const PUBLIC_CACHE_SECONDS = 600;
const FAILURE_STAGES = new Map([
  ["protect", new Set(["prepare", "unpack", "manifest", "dex_runtime", "align", "sign"])],
  ["sign", new Set(["prepare", "align", "execute"])],
  ["task", new Set(["cancelled", "unknown"])],
]);

function corsHeaders(cacheControl = "no-store") {
  return {
    "Access-Control-Allow-Origin": "*",
    "Access-Control-Allow-Headers": "content-type",
    "Access-Control-Allow-Methods": "GET,POST,OPTIONS",
    "Cache-Control": cacheControl,
  };
}

function json(data, status = 200, cacheControl = "no-store") {
  return new Response(JSON.stringify(data), {
    status,
    headers: { ...corsHeaders(cacheControl), "Content-Type": "application/json; charset=utf-8" },
  });
}

function validUuid(value) {
  return typeof value === "string" && /^[0-9a-f]{8}-[0-9a-f-]{27,28}$/i.test(value);
}

function validDate(value) {
  return typeof value === "string" && /^\d{4}-\d{2}-\d{2}$/.test(value);
}

function count(value) {
  return Number.isInteger(value) && value >= 0 && value <= 10000 ? value : 0;
}

export function normalizeFailureCounts(value) {
  if (value === undefined) return [];
  if (!Array.isArray(value)) throw new Error("失败阶段无效");
  const grouped = new Map();
  for (const item of value) {
    const operation = item?.operation;
    const stage = item?.stage;
    if (typeof operation !== "string" || !FAILURE_STAGES.get(operation)?.has(stage)) {
      throw new Error("失败阶段无效");
    }
    if (!Number.isInteger(item.count) || item.count < 0 || item.count > 10000) {
      throw new Error("失败计数无效");
    }
    const key = `${operation}:${stage}`;
    grouped.set(key, Math.min(10000, (grouped.get(key) || 0) + item.count));
  }
  return [...grouped.entries()].map(([key, value]) => {
    const [operation, stage] = key.split(":");
    return { operation, stage, count: value };
  });
}

export function formatTrendResponse(days, data, versions, failureBreakdown) {
  return {
    schema_version: 2,
    window_days: days,
    data,
    versions,
    failure_breakdown: failureBreakdown,
  };
}

async function saveDailyUsage(request, env) {
  let body;
  try {
    body = await request.json();
  } catch {
    return json({ error: "请求数据格式无效" }, 400);
  }

  if (!validUuid(body.anonymous_id) || !validDate(body.usage_date)) {
    return json({ error: "缺少有效的匿名标识或日期" }, 400);
  }
  if (typeof body.app_version !== "string" || body.app_version.length > 32) {
    return json({ error: "版本号无效" }, 400);
  }
  if (typeof body.platform !== "string" || body.platform.length > 16) {
    return json({ error: "平台无效" }, 400);
  }

  const values = Object.fromEntries(
    [...ALLOWED_EVENTS].map((key) => [key, count(body[key])]),
  );
  let failureCounts;
  try {
    failureCounts = normalizeFailureCounts(body.failure_counts);
  } catch (error) {
    return json({ error: error.message }, 400);
  }
  const createdAt = new Date().toISOString();
  const statements = [env.DB.prepare(`
    INSERT INTO daily_usage_v2 (
      anonymous_id, usage_date, app_version, platform, arch,
      app_start_count, protect_start_count, protect_success_count,
      protect_failed_count, sign_success_count, sign_failed_count, created_at
    ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
    ON CONFLICT(anonymous_id, usage_date, app_version) DO UPDATE SET
      platform = excluded.platform,
      arch = excluded.arch,
      app_start_count = excluded.app_start_count,
      protect_start_count = excluded.protect_start_count,
      protect_success_count = excluded.protect_success_count,
      protect_failed_count = excluded.protect_failed_count,
      sign_success_count = excluded.sign_success_count,
      sign_failed_count = excluded.sign_failed_count
  `).bind(
    body.anonymous_id,
    body.usage_date,
    body.app_version,
    body.platform,
    typeof body.arch === "string" ? body.arch.slice(0, 16) : null,
    values.app_start_count,
    values.protect_start_count,
    values.protect_success_count,
    values.protect_failed_count,
    values.sign_success_count,
    values.sign_failed_count,
    createdAt,
  )];
  statements.push(env.DB.prepare(`
    DELETE FROM daily_usage_failure_v2
    WHERE anonymous_id = ? AND usage_date = ? AND app_version = ?
  `).bind(body.anonymous_id, body.usage_date, body.app_version));
  for (const item of failureCounts) {
    statements.push(env.DB.prepare(`
      INSERT INTO daily_usage_failure_v2 (
        anonymous_id, usage_date, app_version, operation, stage, count
      ) VALUES (?, ?, ?, ?, ?, ?)
    `).bind(
      body.anonymous_id,
      body.usage_date,
      body.app_version,
      item.operation,
      item.stage,
      item.count,
    ));
  }
  await env.DB.batch(statements);

  return new Response(null, { status: 204, headers: corsHeaders() });
}

async function queryStats(env, days) {
  const result = await env.DB.prepare(`
    WITH unified_usage AS (
      SELECT anonymous_id, usage_date, app_version, app_start_count, protect_start_count,
        protect_success_count, protect_failed_count, sign_success_count, sign_failed_count
      FROM daily_usage_v2
      WHERE usage_date >= date('now', ?)
      UNION ALL
      SELECT legacy.anonymous_id, legacy.usage_date, legacy.app_version, legacy.app_start_count,
        legacy.protect_start_count, legacy.protect_success_count, legacy.protect_failed_count,
        legacy.sign_success_count, legacy.sign_failed_count
      FROM daily_usage legacy
      WHERE legacy.usage_date >= date('now', ?)
        AND NOT EXISTS (
          SELECT 1 FROM daily_usage_v2 current
          WHERE current.anonymous_id = legacy.anonymous_id
            AND current.usage_date = legacy.usage_date
            AND current.app_version = legacy.app_version
        )
    )
    SELECT usage_date, COUNT(*) AS active_devices,
      SUM(app_start_count) AS app_starts,
      SUM(protect_success_count) AS protect_successes,
      SUM(protect_failed_count) AS protect_failures,
      SUM(sign_success_count) AS sign_successes
    FROM unified_usage
    GROUP BY usage_date
    ORDER BY usage_date ASC
  `).bind(`-${days - 1} days`, `-${days - 1} days`).all();
  return result.results || [];
}

async function queryVersionStats(env, days) {
  const result = await env.DB.prepare(`
    WITH unified_usage AS (
      SELECT anonymous_id, usage_date, app_version, app_start_count, protect_start_count,
        protect_success_count, protect_failed_count, sign_success_count, sign_failed_count
      FROM daily_usage_v2
      WHERE usage_date >= date('now', ?)
      UNION ALL
      SELECT legacy.anonymous_id, legacy.usage_date, legacy.app_version, legacy.app_start_count,
        legacy.protect_start_count, legacy.protect_success_count, legacy.protect_failed_count,
        legacy.sign_success_count, legacy.sign_failed_count
      FROM daily_usage legacy
      WHERE legacy.usage_date >= date('now', ?)
        AND NOT EXISTS (
          SELECT 1 FROM daily_usage_v2 current
          WHERE current.anonymous_id = legacy.anonymous_id
            AND current.usage_date = legacy.usage_date
            AND current.app_version = legacy.app_version
        )
    )
    SELECT usage_date, app_version, COUNT(*) AS active_devices,
      SUM(app_start_count) AS app_starts,
      SUM(protect_start_count) AS protect_starts,
      SUM(protect_success_count) AS protect_successes,
      SUM(protect_failed_count) AS protect_failures,
      SUM(sign_success_count) AS sign_successes,
      SUM(sign_failed_count) AS sign_failures
    FROM unified_usage
    GROUP BY usage_date, app_version
    ORDER BY usage_date ASC, app_version ASC
  `).bind(`-${days - 1} days`, `-${days - 1} days`).all();
  return result.results || [];
}

async function queryFailureBreakdown(env, days) {
  const result = await env.DB.prepare(`
    SELECT usage_date, app_version, operation, stage, SUM(count) AS count
    FROM daily_usage_failure_v2
    WHERE usage_date >= date('now', ?)
    GROUP BY usage_date, app_version, operation, stage
    ORDER BY usage_date ASC, app_version ASC, operation ASC, stage ASC
  `).bind(`-${days - 1} days`).all();
  return result.results || [];
}

async function getStats(request, env) {
  const url = new URL(request.url);
  const days = Math.min(Math.max(Number(url.searchParams.get("days") || 14), 1), 90);
  const [data, versions, failureBreakdown] = await Promise.all([
    queryStats(env, days),
    queryVersionStats(env, days),
    queryFailureBreakdown(env, days),
  ]);
  return json(
    formatTrendResponse(days, data, versions, failureBreakdown),
    200,
    `public, max-age=60, s-maxage=${PUBLIC_CACHE_SECONDS}`,
  );
}

async function getSummary(env) {
  const summary = await getCurrentSummary(env, queryStats(env, 15));
  return json(summary, 200, `public, max-age=60, s-maxage=${PUBLIC_CACHE_SECONDS}`);
}

async function cached(request, ctx, producer) {
  const cache = caches.default;
  const hit = await cache.match(request);
  if (hit) return hit;
  const response = await producer();
  if (response.ok) ctx.waitUntil(cache.put(request, response.clone()));
  return response;
}

export default {
  async fetch(request, env, ctx) {
    if (request.method === "OPTIONS") return new Response(null, { headers: corsHeaders() });
    try {
      const path = new URL(request.url).pathname;
      if (request.method === "POST" && path === "/events/daily") return saveDailyUsage(request, env);
      if (request.method === "GET" && path === "/stats/trend") {
        return cached(request, ctx, () => getStats(request, env));
      }
      if (request.method === "GET" && path === "/stats/summary") {
        return cached(request, ctx, () => getSummary(env));
      }
      return json({ error: "接口不存在" }, 404);
    } catch (error) {
      console.error(error);
      return json({ error: "统计服务暂时不可用" }, 503);
    }
  },
};

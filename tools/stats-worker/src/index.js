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
  await env.DB.prepare(`
    INSERT INTO daily_usage (
      anonymous_id, usage_date, app_version, platform, arch,
      app_start_count, protect_start_count, protect_success_count,
      protect_failed_count, sign_success_count, sign_failed_count, created_at
    ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
    ON CONFLICT(anonymous_id, usage_date) DO UPDATE SET
      app_version = excluded.app_version,
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
    new Date().toISOString(),
  ).run();

  return new Response(null, { status: 204, headers: corsHeaders() });
}

async function queryStats(env, days) {
  const result = await env.DB.prepare(`
    SELECT usage_date, COUNT(*) AS active_devices,
      SUM(app_start_count) AS app_starts,
      SUM(protect_success_count) AS protect_successes,
      SUM(protect_failed_count) AS protect_failures,
      SUM(sign_success_count) AS sign_successes
    FROM daily_usage
    WHERE usage_date >= date('now', ?)
    GROUP BY usage_date
    ORDER BY usage_date ASC
  `).bind(`-${days - 1} days`).all();
  return result.results || [];
}

async function getStats(request, env) {
  const url = new URL(request.url);
  const days = Math.min(Math.max(Number(url.searchParams.get("days") || 14), 1), 90);
  return json(
    { window_days: days, data: await queryStats(env, days) },
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

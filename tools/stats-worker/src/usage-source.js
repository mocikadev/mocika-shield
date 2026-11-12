// 新协议的构建身份及快照入库；身份值只用于分桶，不认证官方来源。
import { normalizeFailureCounts, normalizeFailureReasonCounts } from "./failure-reasons.js";
import { saveDailyCountry } from "./usage-country.js";

const EVENTS = ["app_start_count", "protect_start_count", "protect_success_count",
  "protect_failed_count", "sign_success_count", "sign_failed_count"];
const FIELDS = new Set(["schema_version", "build_identity", "anonymous_id", "usage_date",
  "app_version", "platform", "arch", ...EVENTS, "failure_counts",
  "failure_classifier_version", "failure_reason_counts"]);

export function normalizeBuildIdentity(value) {
  if (!value || typeof value !== "object" || Array.isArray(value)
    || Object.keys(value).length !== 3
    || !["product_name", "application_id", "build_revision"].every(key => Object.hasOwn(value, key))) {
    throw new Error("构建身份字段无效");
  }
  const { product_name: name, application_id: id, build_revision: revision } = value;
  if (typeof name !== "string" || name.length === 0 || name.trim() !== name
    || new TextEncoder().encode(name).byteLength > 128 || /[\p{Cc}\p{Cs}/\\:*?"<>|]/u.test(name)
    || typeof id !== "string" || !/^[A-Za-z0-9.-]{1,128}$/.test(id)
    || (revision !== null && (typeof revision !== "string" || !/^[0-9a-f]{7,40}$/.test(revision)))) {
    throw new Error("构建身份值无效");
  }
  return { product_name: name, application_id: id, build_revision: revision };
}

export async function sourceKey(identity) {
  const value = JSON.stringify([identity.product_name, identity.application_id, identity.build_revision]);
  const digest = await crypto.subtle.digest("SHA-256", new TextEncoder().encode(value));
  return [...new Uint8Array(digest)].map(byte => byte.toString(16).padStart(2, "0")).join("");
}

function normalize(body) {
  if (!body || typeof body !== "object" || Array.isArray(body)
    || body.schema_version !== 3 || Object.keys(body).some(key => !FIELDS.has(key))
    || typeof body.anonymous_id !== "string"
    || !/^[0-9a-f]{8}-[0-9a-f]{4}-[1-5][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i.test(body.anonymous_id)
    || typeof body.usage_date !== "string" || !/^\d{4}-\d{2}-\d{2}$/.test(body.usage_date)
    || !Number.isFinite(Date.parse(body.usage_date))
    || new Date(body.usage_date).toISOString().slice(0, 10) !== body.usage_date
    || typeof body.app_version !== "string" || !/^[0-9A-Za-z.+-]{1,32}$/.test(body.app_version)
    || !["macos", "windows", "linux"].includes(body.platform)
    || typeof body.arch !== "string" || !/^[A-Za-z0-9_]{1,16}$/.test(body.arch)) {
    throw new Error("日统计格式无效");
  }
  const identity = normalizeBuildIdentity(body.build_identity);
  const counts = EVENTS.map(key => {
    const count = body[key];
    if (!Number.isInteger(count) || count < 0 || count > 10000) throw new Error("日统计计数无效");
    return count;
  });
  const stages = normalizeFailureCounts(body.failure_counts);
  const reasons = normalizeFailureReasonCounts(body.failure_reason_counts);
  if ((body.failure_classifier_version !== undefined && body.failure_classifier_version !== 1)
    || (reasons !== undefined && body.failure_classifier_version !== 1)) {
    throw new Error("日统计分类版本无效");
  }
  return { identity, counts, stages, reasons };
}

function response(status) {
  return new Response(null, { status, headers: {
    "Access-Control-Allow-Origin": "*", "Cache-Control": "no-store",
  } });
}

async function readBody(request) {
  if (!request.body) return new Uint8Array();
  const reader = request.body.getReader();
  const chunks = [];
  let length = 0;
  while (true) {
    const { done, value } = await reader.read();
    if (done) break;
    length += value.byteLength;
    if (length > 65536) {
      await reader.cancel();
      return null;
    }
    chunks.push(value);
  }
  const bytes = new Uint8Array(length);
  let offset = 0;
  for (const chunk of chunks) { bytes.set(chunk, offset); offset += chunk.byteLength; }
  return bytes;
}

export async function saveSourceUsage(request, env) {
  const bytes = await readBody(request);
  if (bytes === null) return response(413);
  let body, normalized;
  try {
    body = JSON.parse(new TextDecoder("utf-8", { fatal: true }).decode(bytes));
    normalized = normalize(body);
  } catch {
    return response(400);
  }
  const { identity, counts, stages, reasons } = normalized;
  const key = await sourceKey(identity);
  const binding = [body.anonymous_id, body.usage_date, body.app_version, key];
  const createdAt = new Date().toISOString();
  const updates = EVENTS.map(name => `${name} = MAX(daily_usage_v3.${name}, excluded.${name})`).join(", ");
  const statements = [env.DB.prepare(`
    INSERT INTO daily_usage_v3(anonymous_id,usage_date,app_version,source_key,
      product_name,application_id,build_revision,platform,arch,${EVENTS.join(",")},failure_classifier_version,created_at)
    VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)
    ON CONFLICT(anonymous_id,usage_date,app_version,source_key) DO UPDATE SET
      ${updates}, failure_classifier_version = COALESCE(excluded.failure_classifier_version,daily_usage_v3.failure_classifier_version)
  `).bind(...binding, identity.product_name, identity.application_id, identity.build_revision,
    body.platform, body.arch, ...counts, body.failure_classifier_version ?? null, createdAt)];
  for (const item of stages) {
    statements.push(env.DB.prepare(`
      INSERT INTO daily_usage_failure_v3(anonymous_id,usage_date,app_version,source_key,operation,stage,count)
      VALUES (?,?,?,?,?,?,?) ON CONFLICT(anonymous_id,usage_date,app_version,source_key,operation,stage)
      DO UPDATE SET count=MAX(daily_usage_failure_v3.count,excluded.count)
    `).bind(...binding, item.operation, item.stage, item.count));
  }
  for (const item of reasons || []) {
    statements.push(env.DB.prepare(`
      INSERT INTO daily_usage_failure_reason_v3(anonymous_id,usage_date,app_version,source_key,flow,operation,stage,code,classifier_version,count)
      VALUES (?,?,?,?,?,?,?,?,?,?) ON CONFLICT(anonymous_id,usage_date,app_version,source_key,flow,operation,stage,code,classifier_version)
      DO UPDATE SET count=MAX(daily_usage_failure_reason_v3.count,excluded.count)
    `).bind(...binding, item.flow, item.operation, item.stage, item.code, item.classifier_version, item.count));
  }
  await env.DB.batch(statements);
  await saveDailyCountry(request, env, body.anonymous_id, body.usage_date, createdAt);
  return response(204);
}

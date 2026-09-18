const FIELDS = [
  "submission_id", "schema_version", "notice_version", "package_name", "app_name",
  "app_version_code", "tool_version", "operation", "flow", "success_date",
];
const UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-[1-5][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i;
const PACKAGE_NAME = /^[A-Za-z_][A-Za-z0-9_]*(?:\.[A-Za-z_][A-Za-z0-9_]*)*$/;
const SEMVER_ID = "(?:0|[1-9]\\d*|[0-9A-Za-z-]*[A-Za-z-][0-9A-Za-z-]*)";
const BUILD_ID = "[0-9A-Za-z-]+";
const SEMVER = new RegExp(`^(0|[1-9]\\d*)\\.(0|[1-9]\\d*)\\.(0|[1-9]\\d*)(?:-${SEMVER_ID}(?:\\.${SEMVER_ID})*)?(?:\\+${BUILD_ID}(?:\\.${BUILD_ID})*)?$`);
const MAX_VERSION_CODE = 9223372036854775807n;
const VALID_FLOWS = new Set(["protect:protect", "protect:protect_with_sign", "sign:sign"]);
const encoder = new TextEncoder();

function validUnicode(value) {
  for (let index = 0; index < value.length; index += 1) {
    const code = value.charCodeAt(index);
    if (code >= 0xd800 && code <= 0xdbff) {
      if (index + 1 >= value.length) return false;
      const next = value.charCodeAt(index + 1);
      if (next < 0xdc00 || next > 0xdfff) return false;
      index += 1;
    } else if (code >= 0xdc00 && code <= 0xdfff) return false;
  }
  return true;
}

function utcDay(value) {
  if (typeof value !== "string" || !/^\d{4}-\d{2}-\d{2}$/.test(value)) return null;
  const time = Date.parse(`${value}T00:00:00.000Z`);
  return Number.isFinite(time) && new Date(time).toISOString().slice(0, 10) === value ? time : null;
}

export function normalizeApplicationUsage(value, byteLength, now = new Date()) {
  if (byteLength !== undefined && byteLength > 8192) throw new Error("报告超过8192字节");
  if (!value || typeof value !== "object" || Array.isArray(value)) throw new Error("报告格式无效");
  const keys = Object.keys(value);
  if (keys.length !== FIELDS.length || keys.some((key) => !FIELDS.includes(key))) throw new Error("报告字段无效");
  if (typeof value.submission_id !== "string" || !UUID.test(value.submission_id)) throw new Error("submission_id无效");
  if (value.schema_version !== 1 || value.notice_version !== 1) throw new Error("协议版本无效");
  if (typeof value.package_name !== "string" || value.package_name.length > 255 || !PACKAGE_NAME.test(value.package_name)) throw new Error("package_name无效");
  if (value.app_name !== null) {
    if (typeof value.app_name !== "string" || !validUnicode(value.app_name)
      || [...value.app_name].length > 256 || encoder.encode(value.app_name).byteLength > 1024
      || /\p{Cc}/u.test(value.app_name)) throw new Error("app_name无效");
  }
  if (value.app_version_code !== null) {
    if (typeof value.app_version_code !== "string" || !/^(0|[1-9]\d*)$/.test(value.app_version_code)
      || BigInt(value.app_version_code) > MAX_VERSION_CODE) throw new Error("app_version_code无效");
  }
  if (typeof value.tool_version !== "string" || value.tool_version.length > 64 || !SEMVER.test(value.tool_version)) throw new Error("tool_version无效");
  if (typeof value.operation !== "string" || typeof value.flow !== "string"
    || !VALID_FLOWS.has(`${value.operation}:${value.flow}`)) throw new Error("operation与flow组合无效");
  const successTime = utcDay(value.success_date);
  const today = Date.parse(`${now.toISOString().slice(0, 10)}T00:00:00.000Z`);
  if (successTime === null || successTime < today - 7 * 86400000 || successTime > today + 86400000) throw new Error("success_date无效");
  const submission = Object.fromEntries(FIELDS.map((field) => [field, value[field]]));
  const canonicalJson = JSON.stringify(submission);
  if (encoder.encode(canonicalJson).byteLength > 8192) throw new Error("报告超过8192字节");
  return { submission, canonicalJson };
}

export const APPLICATION_USAGE_FIELDS = FIELDS;

function json(data, status) {
  return new Response(JSON.stringify(data), { status, headers: {
    "Access-Control-Allow-Origin": "*", "Cache-Control": "no-store", "Content-Type": "application/json; charset=utf-8",
  } });
}

async function digestHex(value) {
  const bytes = await crypto.subtle.digest("SHA-256", encoder.encode(value));
  return [...new Uint8Array(bytes)].map((byte) => byte.toString(16).padStart(2, "0")).join("");
}

async function sourceBucket(secret, ip, day) {
  const key = await crypto.subtle.importKey("raw", encoder.encode(secret), { name: "HMAC", hash: "SHA-256" }, false, ["sign"]);
  const signature = await crypto.subtle.sign("HMAC", key, encoder.encode(`application-usage:${day}:${ip}`));
  return [...new Uint8Array(signature)].map((byte) => byte.toString(16).padStart(2, "0")).join("");
}

async function readLimitedBody(request) {
  if (!request.body) return new Uint8Array();
  const reader = request.body.getReader();
  const chunks = []; let length = 0;
  while (true) {
    const { done, value } = await reader.read();
    if (done) break;
    length += value.byteLength;
    if (length > 8192) { await reader.cancel(); return null; }
    chunks.push(value);
  }
  const bytes = new Uint8Array(length); let offset = 0;
  for (const chunk of chunks) { bytes.set(chunk, offset); offset += chunk.byteLength; }
  return bytes;
}

async function saveUnsafe(request, env, now) {
  if (env.APPLICATION_SHARING_ENABLED !== "true") return json({ error: "APPLICATION_SHARING_DISABLED" }, 503);
  if (!env.APPLICATION_SHARE_RATE_SECRET) return json({ error: "APPLICATION_SHARING_UNAVAILABLE" }, 503);
  const bytes = await readLimitedBody(request);
  if (bytes === null) return json({ error: "APPLICATION_USAGE_TOO_LARGE" }, 413);
  const ip = request.headers.get("CF-Connecting-IP");
  if (!ip) return json({ error: "APPLICATION_USAGE_SOURCE_REQUIRED" }, 400);
  let normalized;
  try {
    const parsed = JSON.parse(new TextDecoder("utf-8", { fatal: true }).decode(bytes));
    normalized = normalizeApplicationUsage(parsed, bytes.byteLength, now);
  } catch { return json({ error: "APPLICATION_USAGE_INVALID" }, 400); }
  const id = normalized.submission.submission_id;
  const existing = await env.DB.prepare("SELECT payload_json FROM application_usage_submissions WHERE submission_id = ?").bind(id).first();
  if (existing) return existing.payload_json === normalized.canonicalJson
    ? json({ submission_id: id }, 200) : json({ error: "APPLICATION_USAGE_ID_CONFLICT" }, 409);

  const receivedAt = now.toISOString();
  const minuteStart = `${receivedAt.slice(0, 16)}:00.000Z`;
  const bucket = await sourceBucket(env.APPLICATION_SHARE_RATE_SECRET, ip, receivedAt.slice(0, 10));
  const digest = await digestHex(normalized.canonicalJson);
  const item = normalized.submission;
  try {
    await env.DB.batch([
      env.DB.prepare(`INSERT INTO application_usage_rate_buckets(bucket_key,minute_start,count,updated_at)
        VALUES(?,?,1,?) ON CONFLICT(bucket_key,minute_start) DO UPDATE SET count=count+1,updated_at=excluded.updated_at`).bind(bucket, minuteStart, receivedAt),
      env.DB.prepare(`INSERT INTO application_usage_submissions(
        submission_id,schema_version,notice_version,package_name,app_name,app_version_code,tool_version,
        operation,flow,success_date,received_at,payload_json,payload_sha256) VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?)`).bind(
        item.submission_id, item.schema_version, item.notice_version, item.package_name, item.app_name,
        item.app_version_code, item.tool_version, item.operation, item.flow, item.success_date,
        receivedAt, normalized.canonicalJson, digest,
      ),
    ]);
  } catch (error) {
    const message = String(error?.message || "");
    let raced;
    try {
      raced = await env.DB.prepare("SELECT payload_json FROM application_usage_submissions WHERE submission_id = ?").bind(id).first();
    } catch {
      return json({ error: "APPLICATION_SHARING_UNAVAILABLE" }, 503);
    }
    if (raced) return raced.payload_json === normalized.canonicalJson
      ? json({ submission_id: id }, 200) : json({ error: "APPLICATION_USAGE_ID_CONFLICT" }, 409);
    if (message.includes("CHECK constraint failed") || message.includes("APPLICATION_USAGE_GLOBAL_LIMIT")) return json({ error: "APPLICATION_USAGE_RATE_LIMITED" }, 429);
    return json({ error: "APPLICATION_SHARING_UNAVAILABLE" }, 503);
  }
  return json({ submission_id: id }, 201);
}

export async function saveApplicationUsage(request, env, now = new Date()) {
  try { return await saveUnsafe(request, env, now); }
  catch { return json({ error: "APPLICATION_SHARING_UNAVAILABLE" }, 503); }
}

export async function cleanupApplicationUsage(env) {
  await env.DB.batch([
    env.DB.prepare("DELETE FROM application_usage_submissions WHERE julianday(received_at) < julianday(date('now', '-180 days'))"),
    env.DB.prepare("DELETE FROM application_usage_rate_buckets WHERE julianday(updated_at) < julianday('now', '-24 hours')"),
  ]);
}

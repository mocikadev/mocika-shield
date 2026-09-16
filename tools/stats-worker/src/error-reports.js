import { FAILURE_CODES, validFailureDimensions } from "./failure-reasons.js";

const REPORT_FIELDS = [
  "report_id", "schema_version", "classifier_version", "sanitizer_version", "app_version",
  "build_revision", "build_kind", "occurred_at", "flow", "operation", "stage", "code",
  "platform", "arch", "java_major", "java_vendor", "tool_name", "tool_version", "exit_code",
  "evidence",
];
const BUILDS = new Set(["release", "local", "unknown"]);
const PLATFORMS = new Set(["macos", "windows", "linux", "unknown"]);
const ARCHES = new Set(["aarch64", "x86_64", "x86", "arm", "unknown"]);
const JAVA_VENDORS = new Set(["oracle", "openjdk", "amazon", "adoptium", "azul", "microsoft", "unknown"]);
const TOOLS = new Set(["java", "keytool", "apktool", "apksigner", "zipalign"]);
const SEMVER_IDENTIFIER = "(?:0|[1-9]\\d*|[0-9A-Za-z-]*[A-Za-z-][0-9A-Za-z-]*)";
const APP_VERSION_PATTERN = new RegExp(
  `^(0|[1-9]\\d*)\\.(0|[1-9]\\d*)\\.(0|[1-9]\\d*)(?:-${SEMVER_IDENTIFIER}(?:\\.${SEMVER_IDENTIFIER})*)?$`,
);

function nullableEnum(value, choices, field) {
  if (value !== null && !choices.has(value)) throw new Error(`${field}无效`);
}

export function normalizeErrorReport(value, byteLength) {
  if (byteLength !== undefined && byteLength > 8192) throw new Error("报告超过8192字节");
  if (!value || typeof value !== "object" || Array.isArray(value)) throw new Error("报告格式无效");
  const keys = Object.keys(value);
  if (keys.length !== REPORT_FIELDS.length || keys.some((key) => !REPORT_FIELDS.includes(key))) {
    throw new Error("报告字段无效");
  }
  if (typeof value.report_id !== "string"
    || !/^[0-9a-f]{8}-[0-9a-f]{4}-[1-5][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i.test(value.report_id)) {
    throw new Error("report_id无效");
  }
  if (value.schema_version !== 1 || value.classifier_version !== 1 || value.sanitizer_version !== 1) {
    throw new Error("协议版本无效");
  }
  if (typeof value.app_version !== "string"
    || value.app_version.length < 1
    || value.app_version.length > 64
    || !APP_VERSION_PATTERN.test(value.app_version)) {
    throw new Error("app_version无效");
  }
  if (value.build_revision !== null
    && (typeof value.build_revision !== "string" || !/^[0-9a-f]{7,40}$/.test(value.build_revision))) {
    throw new Error("build_revision无效");
  }
  if (!BUILDS.has(value.build_kind)) throw new Error("build_kind无效");
  if (!Number.isSafeInteger(value.occurred_at) || value.occurred_at < 0) throw new Error("occurred_at无效");
  if (!validFailureDimensions(value.flow, value.operation, value.stage)) throw new Error("失败维度无效");
  if (!FAILURE_CODES.has(value.code)) throw new Error("code无效");
  if (!PLATFORMS.has(value.platform)) throw new Error("platform无效");
  if (!ARCHES.has(value.arch)) throw new Error("arch无效");
  if (value.java_major !== null
    && (!Number.isInteger(value.java_major) || value.java_major < 8 || value.java_major > 99)) {
    throw new Error("java_major无效");
  }
  nullableEnum(value.java_vendor, JAVA_VENDORS, "java_vendor");
  nullableEnum(value.tool_name, TOOLS, "tool_name");
  if (value.tool_version !== null
    && (typeof value.tool_version !== "string" || !/^\d+(?:\.\d+)*$/.test(value.tool_version) || value.tool_version.length > 32)) {
    throw new Error("tool_version无效");
  }
  if (value.exit_code !== null
    && (!Number.isInteger(value.exit_code) || value.exit_code < -2147483648 || value.exit_code > 2147483647)) {
    throw new Error("exit_code无效");
  }
  if (!Array.isArray(value.evidence) || value.evidence.length !== 0) throw new Error("evidence首期必须为空数组");

  const report = Object.fromEntries(REPORT_FIELDS.map((field) => [field, value[field]]));
  const canonicalJson = JSON.stringify(report);
  if (new TextEncoder().encode(canonicalJson).byteLength > 8192) throw new Error("报告超过8192字节");
  return { report, canonicalJson };
}

export const ERROR_REPORT_FIELDS = REPORT_FIELDS;

function json(data, status) {
  return new Response(JSON.stringify(data), {
    status,
    headers: {
      "Access-Control-Allow-Origin": "*",
      "Cache-Control": "no-store",
      "Content-Type": "application/json; charset=utf-8",
    },
  });
}

async function digestHex(value) {
  const bytes = await crypto.subtle.digest("SHA-256", new TextEncoder().encode(value));
  return [...new Uint8Array(bytes)].map((byte) => byte.toString(16).padStart(2, "0")).join("");
}

async function sourceBucket(secret, ip, day) {
  const key = await crypto.subtle.importKey(
    "raw", new TextEncoder().encode(secret), { name: "HMAC", hash: "SHA-256" }, false, ["sign"],
  );
  const signature = await crypto.subtle.sign("HMAC", key, new TextEncoder().encode(`${day}:${ip}`));
  return [...new Uint8Array(signature)].map((byte) => byte.toString(16).padStart(2, "0")).join("");
}

async function readLimitedBody(request, maxBytes) {
  if (!request.body) return new Uint8Array();
  const reader = request.body.getReader();
  const chunks = [];
  let length = 0;
  while (true) {
    const { done, value } = await reader.read();
    if (done) break;
    length += value.byteLength;
    if (length > maxBytes) {
      await reader.cancel();
      return null;
    }
    chunks.push(value);
  }
  const bytes = new Uint8Array(length);
  let offset = 0;
  for (const chunk of chunks) {
    bytes.set(chunk, offset);
    offset += chunk.byteLength;
  }
  return bytes;
}

async function saveErrorReportUnsafe(request, env) {
  if (env.ERROR_REPORTS_ENABLED !== "true") return json({ error: "ERROR_REPORTS_DISABLED" }, 503);
  if (!env.ERROR_REPORT_RATE_SECRET) return json({ error: "ERROR_REPORTS_UNAVAILABLE" }, 503);
  const bytes = await readLimitedBody(request, 8192);
  if (bytes === null) return json({ error: "ERROR_REPORT_TOO_LARGE" }, 413);
  const ip = request.headers.get("CF-Connecting-IP");
  if (!ip) return json({ error: "ERROR_REPORT_SOURCE_REQUIRED" }, 400);
  let parsed;
  try {
    parsed = JSON.parse(new TextDecoder("utf-8", { fatal: true }).decode(bytes));
  } catch {
    return json({ error: "ERROR_REPORT_INVALID" }, 400);
  }
  let normalized;
  try {
    normalized = normalizeErrorReport(parsed, bytes.byteLength);
  } catch {
    return json({ error: "ERROR_REPORT_INVALID" }, 400);
  }

  const existing = await env.DB.prepare(
    "SELECT payload_json FROM error_reports WHERE report_id = ?",
  ).bind(normalized.report.report_id).first();
  if (existing) {
    return existing.payload_json === normalized.canonicalJson
      ? json({ report_id: normalized.report.report_id }, 200)
      : json({ error: "ERROR_REPORT_ID_CONFLICT" }, 409);
  }

  const now = new Date();
  const receivedAt = now.toISOString();
  const minuteStart = `${receivedAt.slice(0, 16)}:00.000Z`;
  const bucket = await sourceBucket(env.ERROR_REPORT_RATE_SECRET, ip, receivedAt.slice(0, 10));
  const payloadSha256 = await digestHex(normalized.canonicalJson);
  const report = normalized.report;
  try {
    await env.DB.batch([
      env.DB.prepare(`
        INSERT INTO error_report_rate_buckets(bucket_key, minute_start, count, updated_at)
        VALUES (?, ?, 1, ?)
        ON CONFLICT(bucket_key, minute_start) DO UPDATE SET count = count + 1, updated_at = excluded.updated_at
      `).bind(bucket, minuteStart, receivedAt),
      env.DB.prepare(`
        INSERT INTO error_reports(
          report_id, schema_version, classifier_version, sanitizer_version, app_version,
          build_revision, build_kind, occurred_at, received_at, flow, operation, stage, code,
          platform, arch, java_major, java_vendor, tool_name, tool_version, exit_code,
          payload_json, payload_sha256
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
      `).bind(
        report.report_id, report.schema_version, report.classifier_version, report.sanitizer_version,
        report.app_version, report.build_revision, report.build_kind, report.occurred_at, receivedAt,
        report.flow, report.operation, report.stage, report.code, report.platform, report.arch,
        report.java_major, report.java_vendor, report.tool_name, report.tool_version, report.exit_code,
        normalized.canonicalJson, payloadSha256,
      ),
    ]);
  } catch (error) {
    const message = String(error?.message || "");
    if (message.includes("CHECK constraint failed") || message.includes("ERROR_REPORT_GLOBAL_LIMIT")) {
      return json({ error: "ERROR_REPORT_RATE_LIMITED" }, 429);
    }
    if (message.includes("UNIQUE constraint failed") && message.includes("error_reports.report_id")) {
      const raced = await env.DB.prepare(
        "SELECT payload_json FROM error_reports WHERE report_id = ?",
      ).bind(report.report_id).first();
      return raced?.payload_json === normalized.canonicalJson
        ? json({ report_id: report.report_id }, 200)
        : json({ error: "ERROR_REPORT_ID_CONFLICT" }, 409);
    }
    return json({ error: "ERROR_REPORTS_UNAVAILABLE" }, 503);
  }
  return json({ report_id: report.report_id }, 201);
}

export async function saveErrorReport(request, env) {
  try {
    return await saveErrorReportUnsafe(request, env);
  } catch {
    return json({ error: "ERROR_REPORTS_UNAVAILABLE" }, 503);
  }
}

export async function cleanupErrorReports(env) {
  await env.DB.batch([
    env.DB.prepare("DELETE FROM error_reports WHERE julianday(received_at) < julianday('now', '-30 days')"),
    env.DB.prepare("DELETE FROM error_report_rate_buckets WHERE julianday(updated_at) < julianday('now', '-24 hours')"),
  ]);
}

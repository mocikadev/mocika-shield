import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";

import worker from "./index.js";

const validReport = {
  report_id: "123e4567-e89b-42d3-a456-426614174000", schema_version: 1,
  classifier_version: 1, sanitizer_version: 1, app_version: "1.4.0-beta.5",
  build_revision: "abcdef0", build_kind: "release", occurred_at: 1789516800,
  flow: "protect_with_sign", operation: "sign", stage: "execute", code: "SIGNING_FAILED",
  platform: "macos", arch: "aarch64", java_major: 21, java_vendor: "adoptium",
  tool_name: "apksigner", tool_version: "35.0.1", exit_code: 1, evidence: [],
};

function literal(value) {
  if (value === null || value === undefined) return "NULL";
  if (typeof value === "number") return String(value);
  return `'${String(value).replaceAll("'", "''")}'`;
}

function expand(sql, values) {
  let index = 0;
  return sql.replaceAll("?", () => literal(values[index++]));
}

class SqliteStatement {
  constructor(db, sql, values = []) { this.db = db; this.sql = sql; this.values = values; }
  bind(...values) { return new SqliteStatement(this.db, this.sql, values); }
  async all() { return { results: this.db.query(expand(this.sql, this.values)) }; }
  async first() { return this.db.query(expand(this.sql, this.values))[0] ?? null; }
  async run() { this.db.exec(expand(this.sql, this.values)); return { success: true }; }
}

class SqliteD1 {
  constructor(path) { this.path = path; }
  prepare(sql) { return new SqliteStatement(this, sql); }
  exec(sql) {
    execFileSync("sqlite3", ["-bail", this.path], {
      input: sql,
      stdio: ["pipe", "pipe", "pipe"],
    });
  }
  query(sql) {
    const output = execFileSync("sqlite3", ["-json", this.path], { input: sql, encoding: "utf8" }).trim();
    return output ? JSON.parse(output) : [];
  }
  queryPlan(sql) {
    return execFileSync("sqlite3", [this.path], { input: `EXPLAIN QUERY PLAN ${sql}`, encoding: "utf8" });
  }
  async batch(statements) {
    this.exec(`BEGIN IMMEDIATE;\n${statements.map((item) => `${expand(item.sql, item.values)};`).join("\n")}\nCOMMIT;`);
    return statements.map(() => ({ success: true }));
  }
}

function request(report = validReport, ip = "203.0.113.7") {
  return new Request("https://stats.invalid/reports/errors", {
    method: "POST",
    headers: { "content-type": "application/json", "CF-Connecting-IP": ip },
    body: JSON.stringify(report),
  });
}

function uuidAt(index) {
  return `123e4567-e89b-42d3-a456-${String(index).padStart(12, "0")}`;
}

test("HTTP错误报告支持幂等、冲突、关闭、大小限制、来源限流与公开读取隔离", async (t) => {
  const dir = mkdtempSync(join(tmpdir(), "shield-stats-"));
  t.after(() => rmSync(dir, { recursive: true, force: true }));
  const db = new SqliteD1(join(dir, "test.db"));
  db.exec(readFileSync(new URL("../schema.sql", import.meta.url), "utf8"));
  const env = { DB: db, ERROR_REPORTS_ENABLED: "true", ERROR_REPORT_RATE_SECRET: "test-secret" };

  assert.equal((await worker.fetch(request(), { ...env, ERROR_REPORTS_ENABLED: "false" }, {})).status, 503);
  assert.equal((await worker.fetch(request(), env, {})).status, 201);
  assert.equal((await worker.fetch(request(), env, {})).status, 200);
  assert.equal((await worker.fetch(request({ ...validReport, code: "UNKNOWN" }), env, {})).status, 409);
  assert.equal((await worker.fetch(request({ ...validReport, report_id: uuidAt(99), raw_error: "secret" }), env, {})).status, 400);
  assert.equal((await worker.fetch(new Request("https://stats.invalid/reports/errors", { method: "POST", body: "x".repeat(8193) }), env, {})).status, 413);
  assert.equal((await worker.fetch(new Request("https://stats.invalid/reports/errors"), env, {})).status, 404);

  for (let index = 1; index <= 9; index += 1) {
    assert.equal((await worker.fetch(request({ ...validReport, report_id: uuidAt(index) }), env, {})).status, 201);
  }
  assert.equal((await worker.fetch(request({ ...validReport, report_id: uuidAt(10) }), env, {})).status, 429);
  assert.equal(db.query("SELECT COUNT(*) AS count FROM error_reports")[0].count, 10);
});

test("真实SQL保留原因最大快照、旧客户端不删除并执行定时清理", async (t) => {
  const dir = mkdtempSync(join(tmpdir(), "shield-stats-"));
  t.after(() => rmSync(dir, { recursive: true, force: true }));
  const db = new SqliteD1(join(dir, "test.db"));
  db.exec(readFileSync(new URL("../schema.sql", import.meta.url), "utf8"));
  const env = { DB: db };
  const base = {
    anonymous_id: "123e4567-e89b-42d3-a456-426614174000", usage_date: "2026-09-16",
    app_version: "1.4.0-beta.5", platform: "macos", arch: "aarch64",
    failure_classifier_version: 1,
    failure_reason_counts: [{ flow: "sign", operation: "sign", stage: "execute", code: "SIGNING_FAILED", classifier_version: 1, count: 5 }],
  };
  const post = (body) => worker.fetch(new Request("https://stats.invalid/events/daily", { method: "POST", body: JSON.stringify(body) }), env, {});
  assert.equal((await post(base)).status, 204);
  assert.equal((await post({ ...base, failure_reason_counts: [{ ...base.failure_reason_counts[0], count: 3 }] })).status, 204);
  assert.equal((await post({ ...base, failure_classifier_version: undefined, failure_reason_counts: undefined })).status, 204);
  assert.equal(db.query("SELECT count FROM daily_usage_failure_reason")[0].count, 5);
  assert.equal(db.query("SELECT failure_classifier_version FROM daily_usage_v2")[0].failure_classifier_version, 1);

  db.exec("INSERT INTO error_reports(report_id,schema_version,classifier_version,sanitizer_version,app_version,build_kind,occurred_at,received_at,flow,operation,stage,code,platform,arch,payload_json,payload_sha256) VALUES('00000000-0000-4000-8000-000000000000',1,1,1,'x','unknown',0,datetime('now','-31 days'),'sign','sign','execute','UNKNOWN','unknown','unknown','{}','x'); INSERT INTO error_report_rate_buckets(bucket_key,minute_start,count,updated_at) VALUES('old','2000-01-01T00:00:00Z',1,'2000-01-01T00:00:00Z');");
  await worker.scheduled({}, env, { waitUntil: (promise) => promise });
  assert.equal(db.query("SELECT COUNT(*) AS count FROM error_reports WHERE report_id='00000000-0000-4000-8000-000000000000'")[0].count, 0);
  assert.equal(db.query("SELECT COUNT(*) AS count FROM error_report_rate_buckets WHERE bucket_key='old'")[0].count, 0);
});

test("定时清理正确处理ISO时间阈值当日的过期记录", async (t) => {
  const dir = mkdtempSync(join(tmpdir(), "shield-stats-cleanup-boundary-"));
  t.after(() => rmSync(dir, { recursive: true, force: true }));
  const db = new SqliteD1(join(dir, "test.db"));
  db.exec(readFileSync(new URL("../schema.sql", import.meta.url), "utf8"));
  db.exec(`
    INSERT INTO error_reports(report_id,schema_version,classifier_version,sanitizer_version,app_version,build_kind,occurred_at,received_at,flow,operation,stage,code,platform,arch,payload_json,payload_sha256)
    VALUES('00000000-0000-4000-8000-000000000001',1,1,1,'x','unknown',0,strftime('%Y-%m-%dT%H:%M:%fZ','now','-30 days','-1 second'),'sign','sign','execute','UNKNOWN','unknown','unknown','{}','x');
    INSERT INTO error_report_rate_buckets(bucket_key,minute_start,count,updated_at)
    VALUES('boundary',strftime('%Y-%m-%dT%H:%M:00.000Z','now','-24 hours','-1 second'),1,strftime('%Y-%m-%dT%H:%M:%fZ','now','-24 hours','-1 second'));
  `);
  await worker.scheduled({}, { DB: db }, { waitUntil: (promise) => promise });
  assert.equal(db.query("SELECT COUNT(*) AS count FROM error_reports")[0].count, 0);
  assert.equal(db.query("SELECT COUNT(*) AS count FROM error_report_rate_buckets")[0].count, 0);
});

test("报告数据库首次查询异常安全返回503且不泄漏异常内容", async () => {
  const secret = "数据库密码不应泄漏";
  const original = console.error;
  const logs = [];
  console.error = (...values) => logs.push(values.join(" "));
  try {
    const response = await worker.fetch(request(), {
      ERROR_REPORTS_ENABLED: "true",
      ERROR_REPORT_RATE_SECRET: "test-secret",
      DB: { prepare() { throw new Error(secret); } },
    }, {});
    assert.equal(response.status, 503);
    assert.equal(logs.some((line) => line.includes(secret)), false);
  } finally {
    console.error = original;
  }
});

test("报告正文流超过8KiB时立即取消且不依赖Content-Length", async () => {
  let pulls = 0;
  let cancelled = false;
  const body = new ReadableStream({
    pull(controller) {
      pulls += 1;
      if (pulls === 4) {
        controller.close();
        return;
      }
      controller.enqueue(new Uint8Array(4096));
    },
    cancel() { cancelled = true; },
  });
  const response = await worker.fetch(new Request("https://stats.invalid/reports/errors", {
    method: "POST",
    headers: { "CF-Connecting-IP": "203.0.113.8" },
    body,
    duplex: "half",
  }), {
    ERROR_REPORTS_ENABLED: "true",
    ERROR_REPORT_RATE_SECRET: "test-secret",
    DB: { prepare() { throw new Error("不应访问数据库"); } },
  }, {});
  assert.equal(response.status, 413);
  assert.equal(cancelled, true);
  assert.equal(pulls, 3);
});

test("真实SQL触发器原子拒绝每日第1001份报告", (t) => {
  const dir = mkdtempSync(join(tmpdir(), "shield-stats-global-limit-"));
  t.after(() => rmSync(dir, { recursive: true, force: true }));
  const db = new SqliteD1(join(dir, "test.db"));
  db.exec(readFileSync(new URL("../schema.sql", import.meta.url), "utf8"));
  db.exec(`
    WITH RECURSIVE sequence(value) AS (
      SELECT 1 UNION ALL SELECT value + 1 FROM sequence WHERE value < 1000
    )
    INSERT INTO error_reports(
      report_id,schema_version,classifier_version,sanitizer_version,app_version,build_kind,
      occurred_at,received_at,flow,operation,stage,code,platform,arch,payload_json,payload_sha256
    ) SELECT printf('00000000-0000-4000-8000-%012d', value),1,1,1,'x','unknown',0,
      '2026-09-16T12:00:00.000Z','sign','sign','execute','UNKNOWN','unknown','unknown','{}',printf('%d', value)
    FROM sequence;
  `);
  assert.throws(() => db.exec(`
    INSERT INTO error_reports(
      report_id,schema_version,classifier_version,sanitizer_version,app_version,build_kind,
      occurred_at,received_at,flow,operation,stage,code,platform,arch,payload_json,payload_sha256
    ) VALUES('ffffffff-ffff-4fff-8fff-ffffffffffff',1,1,1,'x','unknown',0,
      '2026-09-16T23:59:00.000Z','sign','sign','execute','UNKNOWN','unknown','unknown','{}','overflow');
  `), /ERROR_REPORT_GLOBAL_LIMIT/);
  assert.equal(db.query("SELECT COUNT(*) AS count FROM error_reports")[0].count, 1000);
});

test("全局日限额计数使用接收时间索引且保持跨维度计数语义", (t) => {
  const dir = mkdtempSync(join(tmpdir(), "shield-stats-global-plan-"));
  t.after(() => rmSync(dir, { recursive: true, force: true }));
  const db = new SqliteD1(join(dir, "test.db"));
  db.exec(readFileSync(new URL("../schema.sql", import.meta.url), "utf8"));
  const plan = db.queryPlan(`
    SELECT COUNT(*) FROM error_reports
    WHERE received_at >= '2026-09-16T00:00:00.000Z'
      AND received_at < '2026-09-17T00:00:00.000Z';
  `);
  assert.match(plan, /USING COVERING INDEX idx_error_reports_received/);

  db.exec(`
    INSERT INTO error_reports(report_id,schema_version,classifier_version,sanitizer_version,app_version,build_kind,occurred_at,received_at,flow,operation,stage,code,platform,arch,payload_json,payload_sha256) VALUES
      ('00000000-0000-4000-8000-000000000101',1,1,1,'1.0.0','unknown',0,'2026-09-16T01:00:00.000Z','sign','sign','execute','UNKNOWN','unknown','unknown','{}','1'),
      ('00000000-0000-4000-8000-000000000102',1,1,1,'2.0.0','unknown',0,'2026-09-16T02:00:00.000Z','sign','sign','execute','SIGNING_FAILED','unknown','unknown','{}','2'),
      ('00000000-0000-4000-8000-000000000103',1,1,1,'1.0.0','unknown',0,'2026-09-17T01:00:00.000Z','sign','sign','execute','UNKNOWN','unknown','unknown','{}','3');
  `);
  assert.equal(db.query(`
    SELECT COUNT(*) AS count FROM error_reports
    WHERE received_at >= '2026-09-16T00:00:00.000Z'
      AND received_at < '2026-09-17T00:00:00.000Z'
  `)[0].count, 2);
});

test("增量迁移可从第二代统计表建立诊断结构", (t) => {
  const dir = mkdtempSync(join(tmpdir(), "shield-stats-migration-"));
  t.after(() => rmSync(dir, { recursive: true, force: true }));
  const db = new SqliteD1(join(dir, "test.db"));
  db.exec(readFileSync(new URL("../migrations/0001_daily_usage.sql", import.meta.url), "utf8"));
  db.exec(readFileSync(new URL("../migrations/0002_daily_usage_v2.sql", import.meta.url), "utf8"));
  db.exec(readFileSync(new URL("../migrations/0003_failure_diagnostics.sql", import.meta.url), "utf8"));
  assert.deepEqual(
    db.query("SELECT name FROM sqlite_master WHERE type='table' AND name IN ('daily_usage_failure_reason','error_reports','error_report_rate_buckets') ORDER BY name").map((row) => row.name),
    ["daily_usage_failure_reason", "error_report_rate_buckets", "error_reports"],
  );
  assert.equal(db.query("SELECT COUNT(*) AS count FROM pragma_table_info('daily_usage_v2') WHERE name='failure_classifier_version'")[0].count, 1);
});

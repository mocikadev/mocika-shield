import assert from "node:assert/strict";
import { mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";

import { cleanupApplicationUsage, saveApplicationUsage } from "./application-usage.js";
import worker from "./index.js";
import { SqliteD1 } from "./sqlite-d1.test-support.js";

const fixture = JSON.parse(readFileSync(new URL("../../../tests/fixtures/diagnostics/application-usage-v1.json", import.meta.url), "utf8"));
const now = new Date("2026-09-18T12:34:56.000Z");

function setup(t, migrationOnly = false) {
  const dir = mkdtempSync(join(tmpdir(), "shield-application-usage-"));
  t.after(() => rmSync(dir, { recursive: true, force: true }));
  const db = new SqliteD1(join(dir, "test.db"));
  if (migrationOnly) {
    for (const file of ["0001_daily_usage.sql", "0002_daily_usage_v2.sql", "0003_failure_diagnostics.sql", "0004_application_usage.sql"])
      db.exec(readFileSync(new URL(`../migrations/${file}`, import.meta.url), "utf8"));
  } else db.exec(readFileSync(new URL("../schema.sql", import.meta.url), "utf8"));
  return db;
}
function env(db, extra = {}) { return { DB: db, APPLICATION_SHARING_ENABLED: "true", APPLICATION_SHARE_RATE_SECRET: "test-secret", ...extra }; }
function request(body = fixture, ip = "203.0.113.7") {
  return new Request("https://stats.invalid/reports/application-usage", { method: "POST", headers: { "CF-Connecting-IP": ip }, body: JSON.stringify(body) });
}
function uuidAt(index) { return `123e4567-e89b-42d3-a456-${String(index).padStart(12, "0")}`; }

test("应用分享HTTP支持默认关闭、缺密钥、创建、幂等、冲突、非法和无公开读取", async (t) => {
  const db = setup(t);
  assert.equal((await saveApplicationUsage(request(), { DB: db }, now)).status, 503);
  assert.equal((await saveApplicationUsage(request(), env(db, { APPLICATION_SHARE_RATE_SECRET: "" }), now)).status, 503);
  assert.equal((await saveApplicationUsage(request(), env(db), now)).status, 201);
  assert.equal((await saveApplicationUsage(request(), env(db), now)).status, 200);
  assert.equal((await saveApplicationUsage(request({ ...fixture, app_name: "另一个名称" }), env(db), now)).status, 409);
  assert.equal((await saveApplicationUsage(request({ ...fixture, submission_id: uuidAt(2), unknown: "secret" }), env(db), now)).status, 400);
  assert.equal((await worker.fetch(new Request("https://stats.invalid/reports/application-usage"), env(db), {})).status, 404);
  assert.equal(db.query("SELECT COUNT(*) AS count FROM application_usage_submissions")[0].count, 1);
  assert.equal(db.query("SELECT COUNT(*) AS count FROM application_usage_rate_buckets")[0].count, 1);
  const stored = db.query("SELECT payload_json,payload_sha256 FROM application_usage_submissions")[0];
  assert.equal(stored.payload_json, JSON.stringify(fixture));
  assert.match(stored.payload_sha256, /^[0-9a-f]{64}$/);
  assert.equal(stored.payload_json.includes("203.0.113.7"), false);
});

test("正文流超过8KiB立即取消且非法UTF8返回400", async () => {
  let pulls = 0; let cancelled = false;
  const body = new ReadableStream({ pull(controller) { pulls += 1; controller.enqueue(new Uint8Array(4096)); }, cancel() { cancelled = true; } });
  const unavailable = { prepare() { throw new Error("不应访问数据库"); } };
  const tooLarge = await saveApplicationUsage(new Request("https://stats.invalid/reports/application-usage", {
    method: "POST", headers: { "CF-Connecting-IP": "203.0.113.8" }, body, duplex: "half",
  }), env(unavailable), now);
  assert.equal(tooLarge.status, 413); assert.equal(cancelled, true); assert.equal(pulls, 3);
  const invalidUtf8 = await saveApplicationUsage(new Request("https://stats.invalid/reports/application-usage", {
    method: "POST", headers: { "CF-Connecting-IP": "203.0.113.8" }, body: new Uint8Array([0xc3, 0x28]),
  }), env(unavailable), now);
  assert.equal(invalidUtf8.status, 400);
});

test("来源分钟限额20且失败事务不留下桶增长", async (t) => {
  const db = setup(t);
  for (let index = 1; index <= 20; index += 1)
    assert.equal((await saveApplicationUsage(request({ ...fixture, submission_id: uuidAt(index) }), env(db), now)).status, 201);
  assert.equal((await saveApplicationUsage(request({ ...fixture, submission_id: uuidAt(21) }), env(db), now)).status, 429);
  assert.equal(db.query("SELECT count FROM application_usage_rate_buckets")[0].count, 20);
  assert.equal(db.query("SELECT COUNT(*) AS count FROM application_usage_submissions")[0].count, 20);
});

test("同UUID并发重试只写一份且只占一次来源限额", async (t) => {
  const db = setup(t);
  const responses = await Promise.all([
    saveApplicationUsage(request(), env(db), now),
    saveApplicationUsage(request(), env(db), now),
  ]);
  assert.deepEqual(responses.map((response) => response.status).sort(), [200, 201]);
  assert.equal(db.query("SELECT COUNT(*) AS count FROM application_usage_submissions")[0].count, 1);
  assert.equal(db.query("SELECT count FROM application_usage_rate_buckets")[0].count, 1);
});

test("来源第20条的同UUID并发按载荷返回幂等或冲突", async (t) => {
  for (const conflict of [false, true]) {
    const db = setup(t);
    for (let index = 1; index <= 19; index += 1)
      assert.equal((await saveApplicationUsage(request({ ...fixture, submission_id: uuidAt(index) }), env(db), now)).status, 201);
    const id = uuidAt(20);
    const responses = await Promise.all([
      saveApplicationUsage(request({ ...fixture, submission_id: id }), env(db), now),
      saveApplicationUsage(request({ ...fixture, submission_id: id, app_name: conflict ? "冲突名称" : fixture.app_name }), env(db), now),
    ]);
    assert.deepEqual(responses.map((response) => response.status).sort(), conflict ? [201, 409] : [200, 201]);
    assert.equal(db.query("SELECT count FROM application_usage_rate_buckets")[0].count, 20);
    assert.equal(db.query("SELECT COUNT(*) AS count FROM application_usage_submissions")[0].count, 20);
  }
});

test("每日全局限额2000由数据库原子约束且使用接收时间索引", (t) => {
  const db = setup(t);
  db.exec(`WITH RECURSIVE sequence(value) AS (SELECT 1 UNION ALL SELECT value+1 FROM sequence WHERE value<2000)
    INSERT INTO application_usage_submissions(submission_id,schema_version,notice_version,package_name,app_name,app_version_code,tool_version,operation,flow,success_date,received_at,payload_json,payload_sha256)
    SELECT printf('00000000-0000-4000-8000-%012d',value),1,1,'com.example.app',NULL,NULL,'1.4.0','protect','protect','2026-09-18','2026-09-18T12:00:00.000Z','{}',printf('%d',value) FROM sequence;`);
  assert.throws(() => db.exec(`INSERT INTO application_usage_submissions VALUES('ffffffff-ffff-4fff-8fff-ffffffffffff',1,1,'com.example.app',NULL,NULL,'1.4.0','protect','protect','2026-09-18','2026-09-18T13:00:00.000Z','{}','overflow')`), /APPLICATION_USAGE_GLOBAL_LIMIT/);
  assert.match(db.queryPlan("SELECT COUNT(*) FROM application_usage_submissions WHERE received_at >= '2026-09-18T00:00:00.000Z' AND received_at < '2026-09-19T00:00:00.000Z'"), /USING COVERING INDEX idx_application_usage_received/);
});

test("全局第2000条的同UUID并发优先返回幂等或冲突", async (t) => {
  for (const conflict of [false, true]) {
    const db = setup(t);
    db.exec(`WITH RECURSIVE sequence(value) AS (SELECT 1 UNION ALL SELECT value+1 FROM sequence WHERE value<1999)
      INSERT INTO application_usage_submissions(submission_id,schema_version,notice_version,package_name,app_name,app_version_code,tool_version,operation,flow,success_date,received_at,payload_json,payload_sha256)
      SELECT printf('00000000-0000-4000-8000-%012d',value),1,1,'com.prefill',NULL,NULL,'1.4.0','protect','protect','2026-09-18','2026-09-18T10:00:00.000Z','{}',printf('%d',value) FROM sequence;`);
    const responses = await Promise.all([
      saveApplicationUsage(request(fixture, "203.0.113.10"), env(db), now),
      saveApplicationUsage(request({ ...fixture, app_name: conflict ? "冲突名称" : fixture.app_name }, "203.0.113.10"), env(db), now),
    ]);
    assert.deepEqual(responses.map((response) => response.status).sort(), conflict ? [201, 409] : [200, 201]);
    assert.equal(db.query("SELECT COUNT(*) AS count FROM application_usage_submissions")[0].count, 2000);
    assert.equal(db.query("SELECT count FROM application_usage_rate_buckets")[0].count, 1);
  }
});

test("全局第2001条HTTP返回429并回滚来源桶", async (t) => {
  const db = setup(t);
  db.exec(`WITH RECURSIVE sequence(value) AS (SELECT 1 UNION ALL SELECT value+1 FROM sequence WHERE value<2000)
    INSERT INTO application_usage_submissions(submission_id,schema_version,notice_version,package_name,app_name,app_version_code,tool_version,operation,flow,success_date,received_at,payload_json,payload_sha256)
    SELECT printf('00000000-0000-4000-8000-%012d',value),1,1,'com.prefill',NULL,NULL,'1.4.0','protect','protect','2026-09-18','2026-09-18T10:00:00.000Z','{}',printf('%d',value) FROM sequence;`);
  assert.equal((await saveApplicationUsage(request(fixture, "203.0.113.11"), env(db), now)).status, 429);
  assert.equal(db.query("SELECT COUNT(*) AS count FROM application_usage_rate_buckets")[0].count, 0);
});

test("迁移、清理阈值和维护查询按包名并集去重", async (t) => {
  const db = setup(t, true);
  assert.deepEqual(db.query("SELECT name FROM sqlite_master WHERE type='table' AND name LIKE 'application_usage_%' ORDER BY name").map((row) => row.name), ["application_usage_rate_buckets", "application_usage_submissions"]);
  db.exec(`INSERT INTO application_usage_submissions VALUES
    ('00000000-0000-4000-8000-000000000001',1,1,'com.a','旧名',NULL,'1.4.0','protect','protect','2026-01-01','2000-01-01T00:00:00.000Z','{}','1'),
    ('00000000-0000-4000-8000-000000000002',1,1,'com.a','新名','2','1.4.0','protect','protect','2026-09-18','2026-09-18T01:00:00.000Z','{}','2'),
    ('00000000-0000-4000-8000-000000000003',1,1,'com.a','新名','2','1.4.0','sign','sign','2026-09-18','2026-09-18T02:00:00.000Z','{}','3'),
    ('00000000-0000-4000-8000-000000000004',1,1,'com.b',NULL,NULL,'1.4.0','sign','sign','2026-09-18','2026-09-18T03:00:00.000Z','{}','4'),
    ('00000000-0000-4000-8000-000000000005',1,1,'com.a','再次改名','2','1.4.0','protect','protect','2026-09-18','2026-09-18T04:00:00.000Z','{}','5');
    INSERT INTO application_usage_submissions VALUES
    ('00000000-0000-4000-8000-000000000006',1,1,'com.boundary',NULL,NULL,'1.4.0','protect','protect','2026-09-18',strftime('%Y-%m-%dT00:00:00.000Z','now','-180 days'),'{}','6');
    INSERT INTO application_usage_rate_buckets VALUES
    ('old','2000-01-01T00:00:00.000Z',1,'2000-01-01T00:00:00.000Z'),
    ('boundary',strftime('%Y-%m-%dT%H:%M:00.000Z','now','-24 hours','+1 minute'),1,strftime('%Y-%m-%dT%H:%M:00.000Z','now','-24 hours','+1 minute'));`);
  const counts = db.query("SELECT COUNT(DISTINCT CASE WHEN operation='protect' THEN package_name END) AS protected, COUNT(DISTINCT CASE WHEN operation='sign' THEN package_name END) AS signed, COUNT(DISTINCT package_name) AS overall FROM application_usage_submissions")[0];
  assert.deepEqual(counts, { protected: 2, signed: 2, overall: 3 });
  assert.equal(db.query("SELECT COUNT(*) AS count FROM (SELECT DISTINCT package_name,app_version_code,tool_version,operation,flow,success_date FROM application_usage_submissions)")[0].count, 5);
  await cleanupApplicationUsage({ DB: db });
  assert.equal(db.query("SELECT COUNT(*) AS count FROM application_usage_submissions WHERE received_at LIKE '2000-%'")[0].count, 0);
  assert.equal(db.query("SELECT COUNT(*) AS count FROM application_usage_submissions WHERE package_name='com.boundary'")[0].count, 1);
  assert.equal(db.query("SELECT COUNT(*) AS count FROM application_usage_rate_buckets")[0].count, 1);
});

test("存储异常统一503且不打印载荷或数据库详情", async () => {
  const logs = []; const original = console.error; console.error = (...values) => logs.push(values.join(" "));
  try {
    const response = await saveApplicationUsage(request(), env({ prepare() { throw new Error("数据库密码 secret"); } }), now);
    assert.equal(response.status, 503);
    assert.equal(logs.some((line) => line.includes("secret") || line.includes(fixture.package_name)), false);
  } finally { console.error = original; }
});

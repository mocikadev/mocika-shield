import assert from "node:assert/strict";
import { mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import worker from "./index.js";
import { normalizeBuildIdentity, sourceKey } from "./usage-source.js";
import { queryUsage, queryActivityWindows } from "./usage-queries.js";
import { SqliteD1 } from "./sqlite-d1.test-support.js";

function database(t) {
  const directory = mkdtempSync(join(tmpdir(), "shield-usage-source-"));
  t.after(() => rmSync(directory, { recursive: true, force: true }));
  const db = new SqliteD1(join(directory, "test.db"));
  db.exec(readFileSync(new URL("../schema.sql", import.meta.url), "utf8"));
  return db;
}

function payload() {
  return {
    schema_version: 3,
    build_identity: { product_name: "工具甲", application_id: "dev.example.tool", build_revision: "abcdef01" },
    anonymous_id: "123e4567-e89b-42d3-a456-426614174000",
    usage_date: new Date().toISOString().slice(0, 10),
    app_version: "1.5.0-beta.2", platform: "macos", arch: "aarch64",
    app_start_count: 1, protect_start_count: 2, protect_success_count: 1,
    protect_failed_count: 1, sign_success_count: 1, sign_failed_count: 0,
    failure_classifier_version: 1,
    failure_counts: [{ operation: "protect", stage: "manifest", count: 1 }],
    failure_reason_counts: [{ flow: "protect", operation: "protect", stage: "manifest",
      code: "MANIFEST_REBUILD_FAILED", classifier_version: 1, count: 1 }],
  };
}

function post(db, body, path = "/events/daily-v3") {
  return worker.fetch(new Request("https://stats.invalid" + path, {
    method: "POST", body: JSON.stringify(body),
  }), { DB: db }, {});
}

test("构建身份支持中文且拒绝路径控制字符额外字段和超长值", async () => {
  const value = payload().build_identity;
  assert.deepEqual(normalizeBuildIdentity(value), value);
  for (const product_name of ["", " 名称", "名称\n", "/本机/路径", "中".repeat(43), "\ud800"]) {
    assert.throws(() => normalizeBuildIdentity({ ...value, product_name }));
  }
  assert.throws(() => normalizeBuildIdentity({ ...value, application_id: "bad/id" }));
  assert.throws(() => normalizeBuildIdentity({ ...value, build_revision: "PRIVATE" }));
  assert.throws(() => normalizeBuildIdentity({ ...value, path: "/private" }));
  assert.notEqual(await sourceKey(value), await sourceKey({ ...value, product_name: "工具乙" }));
  assert.notEqual(await sourceKey(value), await sourceKey({ ...value, build_revision: null }));
  assert.equal(await sourceKey({ ...value, product_name: "测试工具😀" }),
    "e367c5a1d518162d748d321e80841f443605bb2b018a73a2671e61f2fe9eca15");
});

test("同标识同日同版跨名称或提交隔离且重试与乱序快照不重复计数", async (t) => {
  const db = database(t);
  const body = payload();
  assert.equal((await post(db, { ...body, protect_success_count: 3 })).status, 204);
  assert.equal((await post(db, body)).status, 204);
  assert.equal((await post(db, body)).status, 204);
  for (const identity of [
    { ...body.build_identity, product_name: "工具乙" },
    { ...body.build_identity, build_revision: "abcdef02" },
  ]) assert.equal((await post(db, { ...body, build_identity: identity })).status, 204);
  assert.deepEqual(db.query("SELECT product_name,build_revision,protect_success_count FROM daily_usage_v3 ORDER BY product_name,build_revision"), [
    { product_name: "工具乙", build_revision: "abcdef01", protect_success_count: 1 },
    { product_name: "工具甲", build_revision: "abcdef01", protect_success_count: 3 },
    { product_name: "工具甲", build_revision: "abcdef02", protect_success_count: 1 },
  ]);
  assert.equal(db.query("SELECT COUNT(*) AS count FROM daily_usage_failure_v3")[0].count, 3);
  assert.equal(db.query("SELECT COUNT(*) AS count FROM daily_usage_failure_reason_v3")[0].count, 3);
  assert.equal(db.query("SELECT COUNT(*) AS count FROM daily_usage_v2")[0].count, 0);
  const [today] = await queryUsage({ DB: db }, 1);
  assert.equal(today.active_devices, 1);
  assert.equal(today.protect_successes, 5);
  assert.deepEqual((await queryActivityWindows({ DB: db })).map(row => row.active_devices), [1, 1]);
});

test("旧新入口分段同时保留且失败分类不互删并保持公开响应无身份", async (t) => {
  const db = database(t);
  const body = payload();
  const { schema_version: _schema, build_identity: _identity, ...legacy } = body;
  assert.equal((await post(db, legacy, "/events/daily")).status, 204);
  assert.equal((await post(db, body, "/events/daily")).status, 400);
  assert.equal((await post(db, body)).status, 204);
  assert.equal((await post(db, { ...body, protect_success_count: 2,
    failure_counts: [], failure_reason_counts: [] })).status, 204);
  const [today] = await queryUsage({ DB: db }, 1);
  assert.equal(today.protect_successes, 3);
  assert.equal(today.active_devices, 1);
  assert.equal(db.query("SELECT count FROM daily_usage_failure_v2")[0].count, 1);
  assert.equal(db.query("SELECT count FROM daily_usage_failure_v3")[0].count, 1);
  const previousCaches = globalThis.caches;
  globalThis.caches = { default: { match: async () => undefined, put: async () => {} } };
  t.after(() => { globalThis.caches = previousCaches; });
  const response = await worker.fetch(new Request("https://stats.invalid/stats/trend?days=1"),
    { DB: db }, { waitUntil: promise => promise });
  assert.equal(response.status, 200);
  const value = await response.json();
  assert.equal(value.failure_breakdown[0].count, 2);
  assert.equal(value.failure_reason_breakdown[0].count, 2);
  assert.equal(value.failure_classifier_coverage[0].reporting_devices, 1);
  const text = JSON.stringify(value);
  for (const field of ["product_name", "application_id", "build_revision", "anonymous_id", "source_key"]) {
    assert.equal(text.includes(field), false);
  }
});

test("非法新协议不回退旧写入且查询存储异常返回受控失败", async (t) => {
  const db = database(t);
  const body = payload();
  for (const changed of [
    { schema_version: 2 }, { build_identity: null }, { build_identity: { ...body.build_identity, extra: "PRIVATE" } },
    { usage_date: "2026-02-30" }, { platform: "unknown" }, { app_start_count: -1 }, { raw_log: "PRIVATE" },
  ]) assert.equal((await post(db, { ...body, ...changed })).status, 400);
  assert.equal(db.query("SELECT COUNT(*) AS count FROM daily_usage_v3")[0].count, 0);
  assert.equal(db.query("SELECT COUNT(*) AS count FROM daily_usage_v2")[0].count, 0);
  db.exec("DROP TABLE daily_usage_v3");
  const log = console.error;
  console.error = () => {};
  try { assert.equal((await post(db, body)).status, 503); }
  finally { console.error = log; }
  assert.equal(db.query("SELECT COUNT(*) AS count FROM daily_usage_failure_v3")[0].count, 0);
});

test("新协议限制实际流大小并拒绝无效UTF8", async (t) => {
  const db = database(t);
  let cancelled = false;
  const stream = new ReadableStream({
    pull(controller) { controller.enqueue(new Uint8Array(32769)); },
    cancel() { cancelled = true; },
  });
  const oversized = new Request("https://stats.invalid/events/daily-v3", { method: "POST", body: stream, duplex: "half" });
  assert.equal((await worker.fetch(oversized, { DB: db }, {})).status, 413);
  assert.equal(cancelled, true);
  const invalid = new Request("https://stats.invalid/events/daily-v3", { method: "POST", body: new Uint8Array([0xff]) });
  assert.equal((await worker.fetch(invalid, { DB: db }, {})).status, 400);
  assert.equal((await worker.fetch(new Request("https://stats.invalid/events/daily-v3"), { DB: db }, {})).status, 404);
});

test("新增迁移幂等且不修改旧定义或计数并与完整结构一致", async (t) => {
  const db = database(t);
  db.exec("INSERT INTO daily_usage_v2(anonymous_id,usage_date,app_version,platform,app_start_count,created_at) VALUES('old','2026-11-01','1.4.3','windows',8,'now')");
  const old = db.query("SELECT name,sql FROM sqlite_master WHERE name NOT LIKE '%v3%' AND type IN ('table','index') ORDER BY name");
  const structures = () => db.query("SELECT name,sql FROM sqlite_master WHERE name LIKE '%v3%' AND sql IS NOT NULL ORDER BY name")
    .map(row => ({ ...row, sql: row.sql.replaceAll(/\s+/g, " ") }));
  const complete = structures();
  const migration = readFileSync(new URL("../migrations/0006_usage_build_identity.sql", import.meta.url), "utf8");
  db.exec("DROP TABLE daily_usage_v3; DROP TABLE daily_usage_failure_v3; DROP TABLE daily_usage_failure_reason_v3;");
  db.exec(migration);
  db.exec(migration);
  assert.deepEqual(structures(), complete);
  assert.deepEqual(db.query("SELECT name,sql FROM sqlite_master WHERE name NOT LIKE '%v3%' AND type IN ('table','index') ORDER BY name"), old);
  assert.deepEqual(db.query("SELECT app_start_count FROM daily_usage_v2"), [{ app_start_count: 8 }]);
  for (const table of ["daily_usage_v3", "daily_usage_failure_v3", "daily_usage_failure_reason_v3"]) {
    assert.equal(db.query(`SELECT COUNT(*) AS count FROM ${table}`)[0].count, 0);
  }
  assert.equal((await post(db, payload())).status, 204);
  assert.equal(db.query("SELECT COUNT(*) AS count FROM daily_usage_v3")[0].count, 1);
});

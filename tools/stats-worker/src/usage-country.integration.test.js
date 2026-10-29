import assert from "node:assert/strict";
import { existsSync, mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";

import worker from "./index.js";
import { SqliteD1 } from "./sqlite-d1.test-support.js";
import { saveDailyCountry } from "./usage-country.js";

const id = "123e4567-e89b-42d3-a456-426614174000";

function database(t) {
  const dir = mkdtempSync(join(tmpdir(), "shield-country-"));
  t.after(() => rmSync(dir, { recursive: true, force: true }));
  const db = new SqliteD1(join(dir, "usage.db"));
  db.exec(readFileSync(new URL("../schema.sql", import.meta.url), "utf8"));
  return db;
}

function dailyRequest(overrides = {}, cf = { country: "CN" }, headers = {}) {
  const request = new Request("https://stats.invalid/events/daily", {
    method: "POST", headers,
    body: JSON.stringify({
      anonymous_id: id, usage_date: new Date().toISOString().slice(0, 10),
      app_version: "1.4.3", platform: "windows", app_start_count: 1,
      ...overrides,
    }),
  });
  Object.defineProperty(request, "cf", { value: cf });
  return request;
}

function countries(db) {
  assert.equal(db.query("SELECT COUNT(*) AS n FROM sqlite_master WHERE name='daily_usage_country'")[0].n, 1);
  return db.query("SELECT anonymous_id,observed_date,country_code,observed_at FROM daily_usage_country");
}

test("当天首次有效出口跨版本与网络切换固定且主计数保持最新快照", async (t) => {
  const db = database(t);
  const env = { DB: db, USAGE_COUNTRY_ENABLED: "true" };
  assert.equal((await worker.fetch(dailyRequest(), env, {})).status, 204);
  const first = countries(db);
  assert.equal(first.length, 1);
  assert.equal(first[0].country_code, "CN");
  assert.equal(first[0].observed_date, new Date().toISOString().slice(0, 10));
  assert.equal((await worker.fetch(dailyRequest({ app_start_count: 3 }, { country: "US" }), env, {})).status, 204);
  assert.equal((await worker.fetch(dailyRequest({ app_version: "1.4.2" }, { country: "JP" }), env, {})).status, 204);
  assert.deepEqual(countries(db), first);
  assert.deepEqual(db.query("SELECT app_version,app_start_count FROM daily_usage_v2 ORDER BY app_version"), [
    { app_version: "1.4.2", app_start_count: 1 }, { app_version: "1.4.3", app_start_count: 3 },
  ]);
});

test("缺失及保留代码不采集且不信任正文与请求头伪造地区", async (t) => {
  const db = database(t);
  const env = { DB: db, USAGE_COUNTRY_ENABLED: "true" };
  for (const country of [undefined, null, "", "XX", "T1", "ZZ", "AA", "中国", "USA", 86]) {
    const request = dailyRequest({ country: "US", country_code: "US" }, { country }, { "CF-IPCountry": "US" });
    assert.equal((await worker.fetch(request, env, {})).status, 204);
    assert.deepEqual(countries(db), []);
  }
  assert.equal((await worker.fetch(dailyRequest({}, { country: "cn" }), env, {})).status, 204);
  assert.equal(countries(db)[0].country_code, "CN");
});

test("服务端开关缺失或关闭时不采集但计数正常", async (t) => {
  const db = database(t);
  for (const enabled of [undefined, "false", "TRUE", "1"]) {
    assert.equal((await worker.fetch(dailyRequest(), { DB: db, USAGE_COUNTRY_ENABLED: enabled }, {})).status, 204);
    assert.deepEqual(countries(db), []);
  }
  assert.equal(db.query("SELECT app_start_count FROM daily_usage_v2")[0].app_start_count, 1);
});

test("历史及未来快照不回填本次网络出口", async (t) => {
  const db = database(t);
  const env = { DB: db, USAGE_COUNTRY_ENABLED: "true" };
  for (const usage_date of ["2020-01-01", "2099-01-01"]) {
    assert.equal((await worker.fetch(dailyRequest({ usage_date }), env, {})).status, 204);
    assert.deepEqual(countries(db), []);
  }
  assert.equal(db.query("SELECT COUNT(*) AS n FROM daily_usage_v2")[0].n, 2);
});

test("地区写入失败不改变主计数成功响应且日志不泄漏请求或数据库详情", async (t) => {
  const db = database(t);
  const logs = [];
  t.mock.method(console, "error", (...args) => logs.push(args.join(" ")));
  const env = {
    USAGE_COUNTRY_ENABLED: "true",
    DB: {
      prepare(sql) {
        if (sql.includes("daily_usage_country")) throw new Error("敏感数据库错误与来源地址");
        return db.prepare(sql);
      },
      batch: (statements) => db.batch(statements),
    },
  };
  assert.equal((await worker.fetch(dailyRequest({ protect_success_count: 2 }), env, {})).status, 204);
  assert.equal(db.query("SELECT protect_success_count FROM daily_usage_v2")[0].protect_success_count, 2);
  assert.deepEqual(countries(db), []);
  assert.deepEqual(logs, ["地区统计保存失败"]);
});

test("非法请求和主事务失败不会先写地区", async (t) => {
  const db = database(t);
  const env = { DB: db, USAGE_COUNTRY_ENABLED: "true" };
  assert.equal((await worker.fetch(dailyRequest({ anonymous_id: "invalid" }), env, {})).status, 400);
  t.mock.method(console, "error", () => {});
  const broken = { ...env, DB: { prepare: (sql) => db.prepare(sql), batch() { throw new Error("主事务失败"); } } };
  assert.equal((await worker.fetch(dailyRequest(), broken, {})).status, 503);
  assert.deepEqual(countries(db), []);
  assert.equal(db.query("SELECT COUNT(*) AS n FROM daily_usage_v2")[0].n, 0);
});

test("国家地区没有公开读取路径且旧趋势输出不包含地区", async (t) => {
  const db = database(t);
  const env = { DB: db, USAGE_COUNTRY_ENABLED: "true" };
  await worker.fetch(dailyRequest(), env, {});
  for (const path of ["/stats/countries", "/stats/country", "/daily_usage_country"]) {
    assert.equal((await worker.fetch(new Request(`https://stats.invalid${path}`), env, {})).status, 404);
  }
  const original = Object.getOwnPropertyDescriptor(globalThis, "caches");
  Object.defineProperty(globalThis, "caches", {
    configurable: true, value: { default: { match: async () => undefined, put: async () => {} } },
  });
  t.after(() => original ? Object.defineProperty(globalThis, "caches", original) : delete globalThis.caches);
  const response = await worker.fetch(new Request("https://stats.invalid/stats/trend?days=1"), env, { waitUntil() {} });
  assert.equal(response.status, 200);
  const text = await response.text();
  assert.equal(text.includes("country"), false);
  assert.equal(text.includes(id), false);
});

test("增量迁移可重复执行且不改写原有计数", (t) => {
  const db = database(t);
  db.exec("INSERT INTO daily_usage_v2(anonymous_id,usage_date,app_version,platform,app_start_count,created_at) VALUES('existing','2026-10-28','1.4.2','macos',7,'2026-10-28T01:00:00Z'); DROP TABLE IF EXISTS daily_usage_country;");
  const migration = new URL("../migrations/0005_usage_country.sql", import.meta.url);
  assert.equal(existsSync(migration), true);
  db.exec(readFileSync(migration, "utf8"));
  db.exec(readFileSync(migration, "utf8"));
  assert.deepEqual(countries(db), []);
  assert.equal(db.query("SELECT app_start_count FROM daily_usage_v2")[0].app_start_count, 7);
});

test("接收时间按UTC跨日且不将前一日快照记为新一天地区", async (t) => {
  const db = database(t);
  const env = { DB: db, USAGE_COUNTRY_ENABLED: "true" };
  await saveDailyCountry(dailyRequest(), env, id, "2026-10-29", "2026-10-29T23:59:59.999Z");
  await saveDailyCountry(dailyRequest({}, { country: "US" }), env, id, "2026-10-29", "2026-10-30T00:00:00.000Z");
  await saveDailyCountry(dailyRequest({}, { country: "US" }), env, id, "2026-10-30", "2026-10-30T00:00:00.001Z");
  assert.deepEqual(db.query("SELECT observed_date,country_code FROM daily_usage_country ORDER BY observed_date"), [
    { observed_date: "2026-10-29", country_code: "CN" }, { observed_date: "2026-10-30", country_code: "US" },
  ]);
});

test("关闭采集仍由定时任务清理90个UTC日期以外地区而保留原计数", async (t) => {
  const db = database(t);
  db.exec(`
    INSERT INTO daily_usage_country(anonymous_id,observed_date,country_code,observed_at) VALUES
      ('old',date('now','-90 days'),'CN',datetime('now','-90 days')),
      ('boundary',date('now','-89 days'),'JP',datetime('now','-89 days')),
      ('today',date('now'),'US',datetime('now'));
    INSERT INTO daily_usage_v2(anonymous_id,usage_date,app_version,platform,created_at)
      VALUES('old',date('now','-90 days'),'1.4.3','windows',datetime('now','-90 days'));
  `);
  let pending;
  await worker.scheduled({}, { DB: db, USAGE_COUNTRY_ENABLED: "false" }, { waitUntil(promise) { pending = promise; } });
  await pending;
  assert.deepEqual(db.query("SELECT anonymous_id FROM daily_usage_country ORDER BY anonymous_id"), [
    { anonymous_id: "boundary" }, { anonymous_id: "today" },
  ]);
  assert.equal(db.query("SELECT COUNT(*) AS n FROM daily_usage_v2")[0].n, 1);
});

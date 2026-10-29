import assert from "node:assert/strict";
import { existsSync, mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";

import { SqliteD1 } from "./sqlite-d1.test-support.js";

function database(t) {
  const dir = mkdtempSync(join(tmpdir(), "shield-country-query-"));
  t.after(() => rmSync(dir, { recursive: true, force: true }));
  const db = new SqliteD1(join(dir, "usage.db"));
  db.exec(readFileSync(new URL("../schema.sql", import.meta.url), "utf8"));
  return db;
}

async function distribution(db, start, end, excluded = "self") {
  const path = new URL("../queries/country-distribution.sql", import.meta.url);
  assert.equal(existsSync(path), true);
  const result = await db.prepare(readFileSync(path, "utf8")).bind(start, end, excluded).all();
  return result.results;
}

function seed(db) {
  db.exec(`
    INSERT INTO daily_usage_v2(anonymous_id,usage_date,app_version,platform,created_at) VALUES
      ('a','2026-10-28','1.4.2','windows','x'),
      ('a','2026-10-29','1.4.2','windows','x'),
      ('a','2026-10-29','1.4.3','windows','x'),
      ('b','2026-10-23','1.4.2','macos','x'),
      ('c','2026-10-29','1.4.3','macos','x'),
      ('d','2026-10-29','1.4.3','macos','x'),
      ('g','2026-09-30','1.4.1','windows','x'),
      ('h','2026-09-29','1.4.1','windows','x'),
      ('self','2026-10-29','1.4.3','macos','x'),
      ('future','2099-01-01','1.4.3','macos','x');
    INSERT INTO daily_usage(anonymous_id,usage_date,app_version,platform,created_at) VALUES
      ('a','2026-10-29','1.4.2','windows','x'),
      ('e','2026-10-29','1.3.0','linux','x');
    INSERT INTO daily_usage_country(anonymous_id,observed_date,country_code,observed_at) VALUES
      ('a','2026-10-28','CN','2026-10-28T00:00:01Z'),
      ('a','2026-10-29','US','2026-10-29T00:00:01Z'),
      ('a','2099-01-01','FR','2099-01-01T00:00:01Z'),
      ('b','2026-10-23','JP','2026-10-23T00:00:01Z'),
      ('d','2026-09-01','SG','2026-09-01T00:00:01Z'),
      ('e','2026-10-29','HK','2026-10-29T00:00:01Z'),
      ('g','2026-09-30','GB','2026-09-30T00:00:01Z'),
      ('h','2026-09-29','DE','2026-09-29T00:00:01Z'),
      ('orphan','2026-10-29','CA','2026-10-29T00:00:01Z'),
      ('self','2026-10-29','CN','2026-10-29T00:00:01Z');
  `);
}

test("单日跨版本和旧新表去重且未知进入分母并排除已登记自测", async (t) => {
  const db = database(t);
  seed(db);
  const rows = await distribution(db, "2026-10-29", "2026-10-29");
  assert.deepEqual(rows.map(row => [row.country_code, row.active_instances]), [[null, 2], ["HK", 1], ["US", 1]]);
  for (const row of rows) {
    assert.equal(row.total_active_instances, 4);
    assert.equal(row.located_instances, 2);
    assert.equal(row.coverage_percent, 50);
  }
});

test("七日与三十日只取窗口内最近有效地区且同实例不跨地区重复计数", async (t) => {
  const db = database(t);
  seed(db);
  const week = await distribution(db, "2026-10-23", "2026-10-29");
  assert.deepEqual(week.map(row => [row.country_code, row.active_instances]), [[null, 2], ["HK", 1], ["JP", 1], ["US", 1]]);
  assert.equal(week[0].coverage_percent, 60);
  assert.equal(week.reduce((sum, row) => sum + row.active_instances, 0), 5);
  const month = await distribution(db, "2026-09-30", "2026-10-29");
  assert.deepEqual(month.map(row => [row.country_code, row.active_instances]), [[null, 2], ["GB", 1], ["HK", 1], ["JP", 1], ["US", 1]]);
  assert.equal(month[0].total_active_instances, 6);
  assert.equal(month[0].located_instances, 4);
  assert.equal(month[0].coverage_percent, 66.7);
  assert.equal(JSON.stringify(month).includes("anonymous_id"), false);
});

test("可不排除实例且结束日期不会纳入未来观察和未来活跃", async (t) => {
  const db = database(t);
  seed(db);
  const rows = await distribution(db, "2026-10-23", "2099-01-01", null);
  assert.deepEqual(rows.map(row => [row.country_code, row.active_instances]), [[null, 2], ["CN", 1], ["HK", 1], ["JP", 1], ["US", 1]]);
  assert.equal(rows[0].total_active_instances, 6);
});

test("空数据库和无活跃窗口不把孤立地区观察误计成用户", async (t) => {
  const db = database(t);
  assert.deepEqual(await distribution(db, "2026-10-23", "2026-10-29"), []);
  db.exec("INSERT INTO daily_usage_country VALUES('orphan','2026-10-29','CN','2026-10-29T00:00:01Z');");
  assert.deepEqual(await distribution(db, "2026-10-23", "2026-10-29"), []);
});
